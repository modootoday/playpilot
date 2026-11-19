//! Mailbox message values, mirroring `playpilot` `mail::message`.

use serde::{Deserialize, Serialize};

use crate::domain::time::rfc3339;

pub const MAX_SUBJECT: usize = 200;
pub const MAX_BODY: usize = 16 * 1024;

/// A message as written by its sender.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub from: String,
    pub to: String,
    pub subject: String,
    pub re: Option<String>,
    pub body: String,
}

/// A message as stored in one mailbox, with its receipts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    pub id: u64,
    pub mailbox: String,
    pub created_at: u64,
    pub envelope: Envelope,
    pub read: bool,
    pub acked: bool,
}

/// Read (seen) and ack (handled).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Receipt {
    Read,
    Ack,
}

/// Text form used by the MCP tools, as the local server prints it.
pub fn render(m: &Stored) -> String {
    let e = &m.envelope;
    let state = match (m.read, m.acked) {
        (_, true) => " [acked]",
        (true, false) => " [read]",
        _ => " [new]",
    };
    let mut s = format!(
        "id {} at {} from {} to {}{state}\nsubject: {}\n",
        m.id,
        rfc3339(m.created_at),
        e.from,
        e.to,
        e.subject
    );
    if let Some(re) = &e.re {
        s.push_str(&format!("re: {re}\n"));
    }
    s.push_str(&e.body);
    s
}
