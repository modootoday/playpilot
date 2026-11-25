//! `EventsObject`: the ordered log of inbound events (GitHub deliveries),
//! de-duplicated by delivery id, with bounded retention. `Read` is the
//! cursor read behind the `/mcp` tool `events_read` (`playpilot events tail`).

pub mod store;

use serde::{Deserialize, Serialize};

use store::EventStore;

/// Events kept: the newest `MAX_EVENTS`, none older than `MAX_AGE`.
pub const MAX_EVENTS: usize = 1_000;
pub const MAX_AGE: u64 = 14 * 86_400;
/// Delivery ids are remembered for `MAX_AGE` even after their event is
/// trimmed by count, so a replay inside that window is still a repeat.
pub const SEEN_AGE: u64 = MAX_AGE;
pub const MAX_READ: usize = 200;

/// One stored event, in log order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub seq: u64,
    pub received_at: u64,
    pub ty: String,
    pub json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogCmd {
    /// A verified event; `id` is the dedupe key.
    Record {
        id: String,
        ty: String,
        json: String,
    },
    /// Events after `after` (a `seq`), oldest first; `ty_prefix` filters by type.
    Read {
        after: u64,
        limit: usize,
        ty_prefix: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogReply {
    Recorded { seq: u64 },
    Duplicate,
    Events(Vec<Entry>),
    Error(String),
}

pub fn record(
    store: &dyn EventStore,
    id: &str,
    ty: &str,
    json: &str,
    now: u64,
) -> Result<LogReply, String> {
    let cutoff = now.saturating_sub(MAX_AGE);
    store.forget_seen(now.saturating_sub(SEEN_AGE))?;
    if store.seen(id)? {
        return Ok(LogReply::Duplicate);
    }
    let seq = store.insert(id, now, ty, json)?;
    store.prune(cutoff, MAX_EVENTS)?;
    Ok(LogReply::Recorded { seq })
}

/// A type filter matches the type itself or whole dot-separated prefixes.
pub fn type_matches(ty: &str, prefix: &str) -> bool {
    ty.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

pub fn read(
    store: &dyn EventStore,
    after: u64,
    limit: usize,
    ty_prefix: Option<&str>,
) -> Result<Vec<Entry>, String> {
    let limit = limit.clamp(1, MAX_READ);
    store.after(after, limit, ty_prefix)
}

pub fn apply(store: &dyn EventStore, cmd: LogCmd, now: u64) -> LogReply {
    let result = match cmd {
        LogCmd::Record { id, ty, json } => record(store, &id, &ty, &json, now),
        LogCmd::Read {
            after,
            limit,
            ty_prefix,
        } => read(store, after, limit, ty_prefix.as_deref()).map(LogReply::Events),
    };
    result.unwrap_or_else(LogReply::Error)
}

#[cfg(test)]
mod tests {
    use super::store::Memory;
    use super::*;

    fn rec(s: &Memory, id: &str, ty: &str, now: u64) -> LogReply {
        apply(
            s,
            LogCmd::Record {
                id: id.into(),
                ty: ty.into(),
                json: format!("{{\"id\":\"{id}\"}}"),
            },
            now,
        )
    }

    fn all(s: &Memory) -> Vec<Entry> {
        read(s, 0, MAX_READ, None).unwrap()
    }

    #[test]
    fn a_repeated_delivery_is_stored_once() {
        let s = Memory::default();
        assert_eq!(
            rec(&s, "d-1", "com.github.ping", 10),
            LogReply::Recorded { seq: 1 }
        );
        assert_eq!(rec(&s, "d-1", "com.github.ping", 20), LogReply::Duplicate);
        assert_eq!(all(&s).len(), 1);
    }

    #[test]
    fn keeps_at_most_max_events_and_still_knows_trimmed_ids() {
        let s = Memory::default();
        for i in 0..MAX_EVENTS + 5 {
            rec(&s, &format!("d-{i}"), "com.github.push", 100);
        }
        let mut seen = Vec::new();
        let mut after = 0;
        loop {
            let page = read(&s, after, MAX_READ, None).unwrap();
            let Some(last) = page.last() else { break };
            after = last.seq;
            seen.extend(page);
        }
        assert_eq!(seen.len(), MAX_EVENTS);
        assert_eq!(seen[0].seq, 6);
        assert_eq!(rec(&s, "d-0", "com.github.push", 200), LogReply::Duplicate);
    }

    #[test]
    fn drops_events_and_ids_older_than_max_age() {
        let s = Memory::default();
        rec(&s, "old", "com.github.push", 1_000);
        rec(&s, "new", "com.github.push", 1_001 + MAX_AGE);
        let left = all(&s);
        assert_eq!(left.len(), 1);
        assert!(left[0].json.contains("new"));
        assert_eq!(
            rec(&s, "old", "com.github.push", 1_002 + MAX_AGE),
            LogReply::Recorded { seq: 3 }
        );
    }

    #[test]
    fn reads_in_order_after_a_cursor_with_a_type_filter() {
        let s = Memory::default();
        rec(&s, "a", "com.github.push", 1);
        rec(&s, "b", "com.github.issues.opened", 2);
        rec(&s, "c", "com.github.push", 3);
        let seqs: Vec<u64> = read(&s, 1, 10, None)
            .unwrap()
            .iter()
            .map(|e| e.seq)
            .collect();
        assert_eq!(seqs, vec![2, 3]);
        let pushes = read(&s, 0, 10, Some("com.github.push")).unwrap();
        assert_eq!(pushes.len(), 2);
        assert_eq!(read(&s, 0, 10, Some("com.github.issues")).unwrap().len(), 1);
        assert!(read(&s, 0, 10, Some("com.github.iss")).unwrap().is_empty());
        assert!(type_matches(
            "com.github.issues.opened",
            "com.github.issues"
        ));
        assert!(!type_matches("com.github.issues_x", "com.github.issues"));
        assert_eq!(read(&s, 0, 0, None).unwrap().len(), 1);
    }
}
