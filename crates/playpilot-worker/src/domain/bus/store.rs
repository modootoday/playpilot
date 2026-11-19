//! Port: where `BusObject` keeps mailboxes and events. The Worker uses
//! Durable Object SQLite; tests use `Memory`.

use super::message::{Envelope, Receipt, Stored};

pub trait BusStore {
    /// Every mailbox that has received a message.
    fn mailboxes(&self) -> Result<Vec<String>, String>;
    /// Stores a copy in `mailbox`; returns its id (increasing).
    fn insert(&self, mailbox: &str, created_at: u64, envelope: &Envelope) -> Result<u64, String>;
    /// Messages in `mailbox`, oldest first.
    fn list(&self, mailbox: &str) -> Result<Vec<Stored>, String>;
    fn get(&self, id: u64) -> Result<Option<Stored>, String>;
    fn mark(&self, id: u64, kind: Receipt) -> Result<(), String>;
    /// Records an event once per key; false when the key was seen before.
    fn record_event(&self, key: &str, received_at: u64, json: &str) -> Result<bool, String>;
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
        pub messages: RefCell<Vec<Stored>>,
        pub events: RefCell<BTreeMap<String, String>>,
    }

    impl BusStore for Memory {
        fn mailboxes(&self) -> Result<Vec<String>, String> {
            let mut m: Vec<String> = self
                .messages
                .borrow()
                .iter()
                .map(|s| s.mailbox.clone())
                .collect();
            m.sort();
            m.dedup();
            Ok(m)
        }

        fn insert(
            &self,
            mailbox: &str,
            created_at: u64,
            envelope: &Envelope,
        ) -> Result<u64, String> {
            let mut msgs = self.messages.borrow_mut();
            let id = msgs.len() as u64 + 1;
            msgs.push(Stored {
                id,
                mailbox: mailbox.into(),
                created_at,
                envelope: envelope.clone(),
                read: false,
                acked: false,
            });
            Ok(id)
        }

        fn list(&self, mailbox: &str) -> Result<Vec<Stored>, String> {
            Ok(self
                .messages
                .borrow()
                .iter()
                .filter(|m| m.mailbox == mailbox)
                .cloned()
                .collect())
        }

        fn get(&self, id: u64) -> Result<Option<Stored>, String> {
            Ok(self.messages.borrow().iter().find(|m| m.id == id).cloned())
        }

        fn mark(&self, id: u64, kind: Receipt) -> Result<(), String> {
            let mut msgs = self.messages.borrow_mut();
            let m = msgs
                .iter_mut()
                .find(|m| m.id == id)
                .ok_or("no such message")?;
            match kind {
                Receipt::Read => m.read = true,
                Receipt::Ack => {
                    m.read = true;
                    m.acked = true;
                }
            }
            Ok(())
        }

        fn record_event(&self, key: &str, _at: u64, json: &str) -> Result<bool, String> {
            let mut ev = self.events.borrow_mut();
            if ev.contains_key(key) {
                return Ok(false);
            }
            ev.insert(key.into(), json.into());
            Ok(true)
        }
    }
}
