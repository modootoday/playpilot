//! Port: where `EventsObject` keeps outbound deliveries and disabled
//! targets. The Worker uses Durable Object SQLite; tests use `Memory`.

use super::Delivery;

pub trait DeliveryStore {
    /// Inserts a new delivery unless one for the same webhook and event
    /// exists; true when it was inserted. Assigns `seq`.
    fn enqueue(&self, d: &Delivery) -> Result<bool, String>;
    /// Replaces the delivery with the same `seq`.
    fn save(&self, d: &Delivery) -> Result<(), String>;
    /// Up to `limit` pending deliveries with `next_at <= now`, earliest first.
    fn due(&self, now: u64, limit: usize) -> Result<Vec<Delivery>, String>;
    /// The earliest `next_at` of a pending delivery.
    fn next_due(&self) -> Result<Option<u64>, String>;
    /// Up to `limit` deliveries with `seq > after`, oldest first, of one
    /// webhook when given.
    fn after(
        &self,
        after: u64,
        limit: usize,
        webhook: Option<&str>,
    ) -> Result<Vec<Delivery>, String>;
    /// Deletes finished deliveries updated before `before`, then finished
    /// ones beyond the newest `keep` rows. Pending deliveries stay.
    fn prune(&self, before: u64, keep: usize) -> Result<(), String>;
    /// True when `webhook` at the URL with this fingerprint answered 410.
    fn disabled(&self, webhook: &str, url: &str) -> Result<bool, String>;
    fn disable(&self, webhook: &str, url: &str, at: u64) -> Result<(), String>;
}

#[cfg(test)]
pub use memory::Memory;

#[cfg(test)]
mod memory {
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::outbound::State;

    #[derive(Default)]
    pub struct Memory {
        pub rows: RefCell<Vec<Delivery>>,
        pub off: RefCell<BTreeSet<(String, String)>>,
    }

    impl DeliveryStore for Memory {
        fn enqueue(&self, d: &Delivery) -> Result<bool, String> {
            let mut rows = self.rows.borrow_mut();
            if rows
                .iter()
                .any(|r| r.webhook == d.webhook && r.event_id == d.event_id)
            {
                return Ok(false);
            }
            let mut d = d.clone();
            d.seq = rows.last().map_or(0, |r| r.seq) + 1;
            rows.push(d);
            Ok(true)
        }

        fn save(&self, d: &Delivery) -> Result<(), String> {
            let mut rows = self.rows.borrow_mut();
            let row = rows
                .iter_mut()
                .find(|r| r.seq == d.seq)
                .ok_or("no such delivery")?;
            *row = d.clone();
            Ok(())
        }

        fn due(&self, now: u64, limit: usize) -> Result<Vec<Delivery>, String> {
            let mut due: Vec<Delivery> = self
                .rows
                .borrow()
                .iter()
                .filter(|r| r.state == State::Pending && r.next_at <= now)
                .cloned()
                .collect();
            due.sort_by_key(|r| (r.next_at, r.seq));
            due.truncate(limit);
            Ok(due)
        }

        fn next_due(&self) -> Result<Option<u64>, String> {
            Ok(self
                .rows
                .borrow()
                .iter()
                .filter(|r| r.state == State::Pending)
                .map(|r| r.next_at)
                .min())
        }

        fn after(
            &self,
            after: u64,
            limit: usize,
            webhook: Option<&str>,
        ) -> Result<Vec<Delivery>, String> {
            Ok(self
                .rows
                .borrow()
                .iter()
                .filter(|r| r.seq > after && webhook.is_none_or(|w| r.webhook == w))
                .take(limit)
                .cloned()
                .collect())
        }

        fn prune(&self, before: u64, keep: usize) -> Result<(), String> {
            let mut rows = self.rows.borrow_mut();
            rows.retain(|r| r.state == State::Pending || r.updated_at >= before);
            let finished: Vec<u64> = rows
                .iter()
                .filter(|r| r.state != State::Pending)
                .map(|r| r.seq)
                .collect();
            let drop: BTreeSet<u64> = finished
                .iter()
                .take(finished.len().saturating_sub(keep))
                .copied()
                .collect();
            rows.retain(|r| !drop.contains(&r.seq));
            Ok(())
        }

        fn disabled(&self, webhook: &str, url: &str) -> Result<bool, String> {
            Ok(self.off.borrow().contains(&(webhook.into(), url.into())))
        }

        fn disable(&self, webhook: &str, url: &str, _at: u64) -> Result<(), String> {
            self.off.borrow_mut().insert((webhook.into(), url.into()));
            Ok(())
        }
    }
}
