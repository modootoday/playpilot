//! Outbound event delivery for `Webhook` resources (Standard Webhooks).
//!
//! A stored inbound event is queued once per matching webhook, keyed by
//! (webhook, event id), so a re-delivered event or a retry never yields a
//! second `webhook-id`. `EventsObject` sends due deliveries from its alarm:
//! `claim` signs them and books the attempt, the runtime sends them, and
//! `finish` records the outcome and the next retry. Secrets are read by
//! binding name at send time; a missing one fails the delivery unsent.

pub mod sign;
pub mod store;
pub mod target;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::eventlog::{MAX_AGE, MAX_READ};
use store::DeliveryStore;
use target::{Target, Url, check_url};

/// Waits before retry 1, 2, ...: 30 s, 2 min, 10 min, 1 h, 6 h.
pub const SCHEDULE: [u64; 5] = [30, 120, 600, 3_600, 21_600];
/// The first attempt plus one per retry; then the delivery fails.
pub const MAX_ATTEMPTS: u32 = SCHEDULE.len() as u32 + 1;
/// Each request is aborted after this many milliseconds.
pub const TIMEOUT_MS: u32 = 10_000;
/// Deliveries sent per alarm run; more due ones re-arm the alarm at once.
pub const BATCH: usize = 20;
/// Finished deliveries kept: the newest `KEEP`, none older than 14 days.
pub const KEEP: usize = 1_000;
/// When the last attempt's outcome was never recorded, give up after this.
pub const LAST_GRACE: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Pending,
    Delivered,
    Failed,
}

/// One event for one webhook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delivery {
    /// Position in the delivery log (assigned by the store).
    pub seq: u64,
    /// The `webhook-id` header, the same on every attempt.
    pub id: String,
    pub webhook: String,
    pub event_id: String,
    pub ty: String,
    /// The stored CloudEvent JSON, sent as is.
    pub body: String,
    pub created_at: u64,
    pub attempts: u32,
    /// When a pending delivery is next due.
    pub next_at: u64,
    pub state: State,
    pub last_status: Option<u16>,
    pub detail: Option<String>,
    pub updated_at: u64,
}

impl Delivery {
    pub fn new(webhook: &str, event_id: &str, ty: &str, body: &str, now: u64) -> Self {
        Self {
            seq: 0,
            id: sign::message_id(webhook, event_id),
            webhook: webhook.into(),
            event_id: event_id.into(),
            ty: ty.into(),
            body: body.into(),
            created_at: now,
            attempts: 0,
            next_at: now,
            state: State::Pending,
            last_status: None,
            detail: None,
            updated_at: now,
        }
    }

    fn fail(&mut self, status: Option<u16>, why: String, now: u64) {
        self.state = State::Failed;
        self.last_status = status;
        self.detail = Some(why);
        self.updated_at = now;
    }

    fn retry_or_fail(&mut self, status: Option<u16>, why: String, now: u64) {
        match retry_after(self.attempts) {
            Some(wait) => {
                self.next_at = now + wait;
                self.last_status = status;
                self.detail = Some(why);
                self.updated_at = now;
            }
            None => {
                let n = self.attempts;
                self.fail(status, format!("gave up after {n} attempts: {why}"), now);
            }
        }
    }
}

/// The wait before the next attempt after `attempts` failed ones, or `None`
/// when that was the last.
pub fn retry_after(attempts: u32) -> Option<u64> {
    let i = usize::try_from(attempts.checked_sub(1)?).ok()?;
    SCHEDULE.get(i).copied()
}

/// A signed request ready to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: String,
    /// Fingerprint of the URL, the key a 410 disables.
    pub fingerprint: String,
}

/// What one attempt produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Status(u16),
    /// No response: network error, refused redirect or timeout.
    Transport(String),
}

/// What the runtime gives `claim`: the targets of the current config and a
/// lookup of Worker secret bindings by name.
pub struct Bindings<'a> {
    pub targets: &'a [Target],
    pub secret: &'a dyn Fn(&str) -> Option<String>,
}

fn fingerprint(url: &str) -> String {
    Sha256::digest(url.as_bytes())[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Queues `event_id` for each webhook name; returns how many are new.
pub fn enqueue(
    outbox: &dyn DeliveryStore,
    webhooks: &[String],
    event_id: &str,
    ty: &str,
    body: &str,
    now: u64,
) -> Result<usize, String> {
    let mut queued = 0;
    for w in webhooks {
        if outbox.enqueue(&Delivery::new(w, event_id, ty, body, now))? {
            queued += 1;
        }
    }
    Ok(queued)
}

/// The signed request for `d`, or why it must not be sent.
fn prepare(d: &Delivery, b: &Bindings, now: u64) -> Result<Outgoing, String> {
    let t = b
        .targets
        .iter()
        .find(|t| t.name == d.webhook)
        .ok_or("the webhook is no longer configured, or it is paused")?;
    let url = match &t.url {
        Url::Literal(u) => u.clone(),
        Url::Secret(name) => (b.secret)(name).ok_or(format!("url binding {name} is not set"))?,
    };
    check_url(url.trim())?;
    let secret = (b.secret)(&t.secret).ok_or(format!(
        "secret binding {} is not set; nothing was sent",
        t.secret
    ))?;
    let key = sign::secret_key(&secret).ok_or(format!(
        "secret binding {} is not a whsec_<base64> secret",
        t.secret
    ))?;
    let signature = sign::signature(&key, &d.id, now, &d.body);
    Ok(Outgoing {
        fingerprint: fingerprint(url.trim()),
        url: url.trim().to_string(),
        headers: vec![
            ("content-type", "application/json".into()),
            ("webhook-id", d.id.clone()),
            ("webhook-timestamp", now.to_string()),
            ("webhook-signature", signature),
        ],
        body: d.body.clone(),
    })
}

/// Takes up to `limit` due deliveries, at most one per webhook so a target
/// sees them in order and a 410 stops the rest; the others stay due and the
/// runtime re-arms the alarm at once. Those that may not be sent (no
/// target, no secret, a disabled target, no outcome after the last attempt)
/// fail here. The rest are booked as attempted, with the next retry already
/// scheduled in case the outcome is never recorded, and returned to send.
pub fn claim(
    outbox: &dyn DeliveryStore,
    b: &Bindings,
    now: u64,
    limit: usize,
) -> Result<Vec<(Delivery, Outgoing)>, String> {
    let mut out: Vec<(Delivery, Outgoing)> = Vec::new();
    for mut d in outbox.due(now, limit)? {
        if out.iter().any(|(o, _)| o.webhook == d.webhook) {
            continue;
        }
        if d.attempts >= MAX_ATTEMPTS {
            let n = d.attempts;
            d.fail(
                d.last_status,
                format!("gave up after {n} attempts: the last outcome was not recorded"),
                now,
            );
            outbox.save(&d)?;
            continue;
        }
        let request = match prepare(&d, b, now) {
            Ok(r) if outbox.disabled(&d.webhook, &r.fingerprint)? => {
                Err("the target answered 410 Gone and is disabled until its url changes".into())
            }
            other => other,
        };
        match request {
            Err(why) => {
                d.fail(None, why, now);
                outbox.save(&d)?;
            }
            Ok(r) => {
                d.attempts += 1;
                d.next_at = now + retry_after(d.attempts).unwrap_or(LAST_GRACE);
                d.updated_at = now;
                outbox.save(&d)?;
                out.push((d, r));
            }
        }
    }
    Ok(out)
}

/// Records the outcome of an attempt `claim` returned: 2xx delivers, 410
/// disables the target and fails, anything else retries on `SCHEDULE` or,
/// after the last attempt, fails.
pub fn finish(
    outbox: &dyn DeliveryStore,
    mut d: Delivery,
    sent: &Outgoing,
    outcome: Outcome,
    now: u64,
) -> Result<Delivery, String> {
    match outcome {
        Outcome::Status(s) if (200..300).contains(&s) => {
            d.state = State::Delivered;
            d.last_status = Some(s);
            d.detail = None;
            d.updated_at = now;
        }
        Outcome::Status(410) => {
            outbox.disable(&d.webhook, &sent.fingerprint, now)?;
            d.fail(
                Some(410),
                "410 Gone: the target is disabled until its url changes".into(),
                now,
            );
        }
        Outcome::Status(s) => d.retry_or_fail(Some(s), format!("HTTP {s}"), now),
        Outcome::Transport(e) => d.retry_or_fail(None, e, now),
    }
    outbox.save(&d)?;
    outbox.prune(now.saturating_sub(MAX_AGE), KEEP)?;
    Ok(d)
}

/// Deliveries after `after` (a `seq`), oldest first, without their bodies.
pub fn read(
    outbox: &dyn DeliveryStore,
    after: u64,
    limit: usize,
    webhook: Option<&str>,
) -> Result<Vec<Delivery>, String> {
    let mut list = outbox.after(after, limit.clamp(1, MAX_READ), webhook)?;
    for d in &mut list {
        d.body.clear();
    }
    Ok(list)
}

#[cfg(test)]
mod tests;
