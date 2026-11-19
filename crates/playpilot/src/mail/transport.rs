//! Port: where mailboxes live: GitHub pull requests or the playpilot Worker.

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
    /// A transport that copies broadcasts itself returns the copies here;
    /// `None` means the use case copies into each mailbox from `actors`.
    fn broadcast(&self, _envelope: &Envelope) -> Option<Result<Vec<(String, u64)>, String>> {
        None
    }
    /// Every actor that has a mailbox, `all` included.
    fn actors(&self) -> Result<Vec<String>, String>;
    /// Lists the messages in `actor`'s mailbox, oldest first.
    fn list(&self, actor: &str) -> Result<Vec<Stored>, String>;
    /// Records a receipt on a message.
    fn receipt(&self, id: u64, kind: Receipt) -> Result<(), String>;
}
