//! Fleet mailbox: actors exchange short messages with read and ack receipts.
//!
//! Layers: `message` (domain), `transport` (port), `github` (infrastructure),
//! and the use cases below. The model follows the pmcp mailbox design
//! (actors, send, inbox, read, ack, broadcast); nothing is shared with pmcp.

pub mod cli;
pub mod github;
pub mod message;
pub mod transport;

use message::{Envelope, Stored, valid_actor};
use transport::{Receipt, Transport};

/// The broadcast mailbox every actor also reads.
pub const BROADCAST: &str = "all";

pub fn send(t: &dyn Transport, envelope: Envelope) -> Result<u64, String> {
    for actor in [&envelope.from, &envelope.to] {
        if !valid_actor(actor) {
            return Err(format!("`{actor}` is not a valid actor name"));
        }
    }
    if envelope.subject.trim().is_empty() {
        return Err("a subject is required".into());
    }
    if envelope.body.trim().is_empty() {
        return Err("a message body is required".into());
    }
    t.send(&envelope)
}

/// Messages for `actor`, oldest first: its own mailbox (without `all`, only
/// messages not acked yet) plus broadcasts created at or after `broadcast_since`
/// (an RFC 3339 UTC timestamp). Receipts on a broadcast are shared by every
/// actor that uses the same account, so broadcasts are filtered by age, not by ack.
pub fn inbox(
    t: &dyn Transport,
    actor: &str,
    all: bool,
    broadcast_since: &str,
) -> Result<Vec<Stored>, String> {
    if !valid_actor(actor) {
        return Err(format!("`{actor}` is not a valid actor name"));
    }
    let mut messages: Vec<Stored> = t
        .list(actor)?
        .into_iter()
        .filter(|m| all || !m.acked)
        .collect();
    if actor != BROADCAST {
        messages.extend(t.list(BROADCAST)?.into_iter().filter(|m| {
            m.envelope.from != actor && (all || m.created_at.as_str() >= broadcast_since)
        }));
    }
    messages.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    Ok(messages)
}

/// RFC 3339 UTC timestamp `days` days before `now_secs` (Unix seconds).
pub fn days_ago(now_secs: u64, days: u64) -> String {
    let secs = now_secs.saturating_sub(days * 86_400);
    let (z, rem) = ((secs / 86_400) as i64 + 719_468, secs % 86_400);
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

pub fn mark(t: &dyn Transport, id: u64, kind: Receipt) -> Result<(), String> {
    t.receipt(id, kind)
}
