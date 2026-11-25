//! Port: where `EventsObject` keeps the log. The Worker uses Durable Object
//! SQLite; tests use `Memory`.

use super::Entry;

pub trait EventStore {
    /// True when `id` was recorded and not yet forgotten.
    fn seen(&self, id: &str) -> Result<bool, String>;
    /// Stores a new event and remembers its id; returns its `seq` (increasing).
    fn insert(&self, id: &str, received_at: u64, ty: &str, json: &str) -> Result<u64, String>;
    /// Deletes events received before `before`, then all but the newest `keep`.
    fn prune(&self, before: u64, keep: usize) -> Result<(), String>;
    /// Forgets ids recorded before `before`.
    fn forget_seen(&self, before: u64) -> Result<(), String>;
    /// Up to `limit` events with `seq > after`, oldest first, whose type
    /// passes `type_matches` when a prefix is given.
    fn after(
        &self,
        after: u64,
        limit: usize,
        ty_prefix: Option<&str>,
    ) -> Result<Vec<Entry>, String>;
}

#[cfg(test)]
pub use memory::Memory;

#[cfg(test)]
mod memory {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Default)]
    pub struct Memory {
        pub events: RefCell<Vec<Entry>>,
        pub seen: RefCell<BTreeMap<String, u64>>,
        pub next: RefCell<u64>,
    }

    impl EventStore for Memory {
        fn seen(&self, id: &str) -> Result<bool, String> {
            Ok(self.seen.borrow().contains_key(id))
        }

        fn insert(&self, id: &str, received_at: u64, ty: &str, json: &str) -> Result<u64, String> {
            let mut next = self.next.borrow_mut();
            *next += 1;
            self.seen.borrow_mut().insert(id.into(), received_at);
            self.events.borrow_mut().push(Entry {
                seq: *next,
                received_at,
                ty: ty.into(),
                json: json.into(),
            });
            Ok(*next)
        }

        fn prune(&self, before: u64, keep: usize) -> Result<(), String> {
            let mut ev = self.events.borrow_mut();
            ev.retain(|e| e.received_at >= before);
            let extra = ev.len().saturating_sub(keep);
            ev.drain(..extra);
            Ok(())
        }

        fn forget_seen(&self, before: u64) -> Result<(), String> {
            self.seen.borrow_mut().retain(|_, at| *at >= before);
            Ok(())
        }

        fn after(
            &self,
            after: u64,
            limit: usize,
            ty_prefix: Option<&str>,
        ) -> Result<Vec<Entry>, String> {
            Ok(self
                .events
                .borrow()
                .iter()
                .filter(|e| {
                    e.seq > after
                        && ty_prefix.is_none_or(|p| crate::domain::eventlog::type_matches(&e.ty, p))
                })
                .take(limit)
                .cloned()
                .collect())
        }
    }
}
