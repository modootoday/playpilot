//! Port: where mailboxes live. GitHub today; other transports implement the same trait.

use super::message::{Envelope, Stored};

/// Receipt kinds, as in a mailbox: read (seen) and ack (handled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receipt {
    Read,
    Ack,
}

pub trait Transport {
    /// Delivers `envelope` to `mailbox`; returns the message id.
    fn send(&self, mailbox: &str, envelope: &Envelope) -> Result<u64, String>;
    /// Every actor that has a mailbox, `all` included.
    fn actors(&self) -> Result<Vec<String>, String>;
    /// Lists the messages in `actor`'s mailbox, oldest first.
    fn list(&self, actor: &str) -> Result<Vec<Stored>, String>;
    /// Records a receipt on a message.
    fn receipt(&self, id: u64, kind: Receipt) -> Result<(), String>;
}
