//! Mailbox bus, with the semantics of `playpilot` `mail`: send to one actor,
//! broadcast (`all`) copies into every other actor's own mailbox, the inbox
//! lists unacked mail oldest first, and read/ack receipts mark handling.

pub mod message;
pub mod store;

use serde::{Deserialize, Serialize};

use crate::domain::ids::valid_name;
use crate::domain::wake::{Due, should_wake};
use message::{Envelope, MAX_BODY, MAX_SUBJECT, Receipt, Stored};
use store::BusStore;

pub const BROADCAST: &str = "all";

/// Sends `envelope`; `directory` lists the actors known from the identity
/// map. Returns `(mailbox, id)` for every copy delivered.
pub fn send(
    store: &dyn BusStore,
    directory: &[String],
    envelope: &Envelope,
    now: u64,
) -> Result<Vec<(String, u64)>, String> {
    for actor in [&envelope.from, &envelope.to] {
        if !valid_name(actor) {
            return Err(format!("`{actor}` is not a valid actor name"));
        }
    }
    if envelope.from == BROADCAST {
        return Err("`all` cannot send".into());
    }
    if envelope.subject.trim().is_empty() || envelope.subject.len() > MAX_SUBJECT {
        return Err(format!("a subject of 1 to {MAX_SUBJECT} bytes is required"));
    }
    if envelope.body.trim().is_empty() || envelope.body.len() > MAX_BODY {
        return Err(format!(
            "a message body of 1 to {MAX_BODY} bytes is required"
        ));
    }
    if envelope.to != BROADCAST {
        return Ok(vec![(
            envelope.to.clone(),
            store.insert(&envelope.to, now, envelope)?,
        )]);
    }
    let mut targets: Vec<String> = directory
        .iter()
        .cloned()
        .chain(store.mailboxes()?)
        .filter(|a| a != BROADCAST && *a != envelope.from && valid_name(a))
        .collect();
    targets.sort();
    targets.dedup();
    if targets.is_empty() {
        return Err("no other actor has a mailbox".into());
    }
    targets
        .into_iter()
        .map(|actor| store.insert(&actor, now, envelope).map(|id| (actor, id)))
        .collect()
}

/// `actor`'s mailbox, oldest first; acked messages only with `all`.
pub fn inbox(store: &dyn BusStore, actor: &str, all: bool) -> Result<Vec<Stored>, String> {
    if !valid_name(actor) {
        return Err(format!("`{actor}` is not a valid actor name"));
    }
    let mut messages: Vec<Stored> = store
        .list(actor)?
        .into_iter()
        .filter(|m| all || !m.acked)
        .collect();
    messages.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    Ok(messages)
}

/// Marks a message in `actor`'s own mailbox; another actor's copy is refused.
/// Once nothing is left unacked, the actor's wake debounce starts over.
pub fn mark(store: &dyn BusStore, actor: &str, id: u64, kind: Receipt) -> Result<(), String> {
    match store.get(id)? {
        Some(m) if m.mailbox == actor => store.mark(id, kind)?,
        _ => return Err(format!("message {id} is not in {actor}'s mailbox")),
    }
    if kind == Receipt::Ack && inbox(store, actor, false)?.is_empty() {
        store.set_last_wake(actor, None)?;
    }
    Ok(())
}

/// The recipients among `sent` that have a routine (`wakeable`) and are due
/// a fire. Each fire time is recorded before the caller fires, so a failed or
/// rate-limited fire also waits out the debounce window.
pub fn due_wakes(
    store: &dyn BusStore,
    sent: &[(String, u64)],
    wakeable: &[String],
    now: u64,
) -> Result<Vec<Due>, String> {
    let mut actors: Vec<&String> = sent
        .iter()
        .map(|(a, _)| a)
        .filter(|a| wakeable.contains(a))
        .collect();
    actors.sort();
    actors.dedup();
    let mut due = Vec::new();
    for actor in actors {
        let unacked = inbox(store, actor, false)?.len();
        if should_wake(store.last_wake(actor)?, now, unacked) {
            store.set_last_wake(actor, Some(now))?;
            due.push(Due {
                actor: actor.clone(),
                unacked,
            });
        }
    }
    Ok(due)
}

/// One operation sent from the router to `BusObject`. `actor` is always the
/// caller's authenticated actor, never a request argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BusCmd {
    Send {
        envelope: Envelope,
        directory: Vec<String>,
        /// Actors that have a routine to fire on new mail.
        #[serde(default)]
        wakeable: Vec<String>,
    },
    Inbox {
        actor: String,
        all: bool,
    },
    Mark {
        actor: String,
        id: u64,
        kind: Receipt,
    },
    /// Validated CloudEvents as `(dedupe key, JSON)`.
    Publish {
        events: Vec<(String, String)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BusReply {
    /// Copies delivered as `(mailbox, id)`, and the actors to wake now.
    Sent {
        copies: Vec<(String, u64)>,
        wake: Vec<Due>,
    },
    Messages(Vec<Stored>),
    Marked,
    /// How many events were new (the rest were repeats).
    Published(usize),
    Error(String),
}

pub fn apply(store: &dyn BusStore, cmd: BusCmd, now: u64) -> BusReply {
    let result = match cmd {
        BusCmd::Send {
            envelope,
            directory,
            wakeable,
        } => send(store, &directory, &envelope, now).and_then(|copies| {
            let wake = due_wakes(store, &copies, &wakeable, now)?;
            Ok(BusReply::Sent { copies, wake })
        }),
        BusCmd::Inbox { actor, all } => inbox(store, &actor, all).map(BusReply::Messages),
        BusCmd::Mark { actor, id, kind } => {
            mark(store, &actor, id, kind).map(|()| BusReply::Marked)
        }
        BusCmd::Publish { events } => events
            .iter()
            .try_fold(0, |n, (key, json)| {
                store
                    .record_event(key, now, json)
                    .map(|new| n + usize::from(new))
            })
            .map(BusReply::Published),
    };
    result.unwrap_or_else(BusReply::Error)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
