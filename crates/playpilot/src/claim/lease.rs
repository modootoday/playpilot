//! Port: leases on the playpilot Worker (a Durable Object owns every lease).
//!
//! With the Worker backend the lease decides who holds an item; the item's
//! claim comment and `claimed` label only mirror it.

/// A lease the Worker granted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// The claim id the caller's credential maps to.
    pub holder: String,
    pub lease: String,
    pub lane: String,
    /// RFC 3339 UTC.
    pub until: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acquired {
    Granted(Grant),
    /// Held by someone else, paused or at capacity: the Worker's reason.
    Refused(String),
}

pub trait Leases {
    /// Takes (or renews, for the same holder) the lease on `item`.
    fn acquire(&self, item: &str, lane: &str, ttl_hours: u64) -> Result<Acquired, String>;
    /// Frees this caller's lease on `item`; a lease that is already gone is not an error.
    fn release(&self, item: &str) -> Result<(), String>;
}

/// `claimed <item> as <holder> (lease <id>, lane <lane>, until <time>)`.
pub fn parse_grant(text: &str) -> Option<Grant> {
    let rest = text.trim().strip_prefix("claimed ")?;
    let (_, rest) = rest.split_once(" as ")?;
    let (holder, rest) = rest.split_once(" (lease ")?;
    let (lease, rest) = rest.split_once(", lane ")?;
    let (lane, rest) = rest.split_once(", until ")?;
    let until = rest.strip_suffix(')')?;
    super::parse_utc(until)?;
    let word = |s: &str| !s.is_empty() && !s.contains(char::is_whitespace);
    if !(word(holder) && word(lease) && word(lane)) {
        return None;
    }
    Some(Grant {
        holder: holder.into(),
        lease: lease.into(),
        lane: lane.into(),
        until: until.into(),
    })
}

/// `not claimed <item>: <reason>` is a refusal; any other error text is not.
pub fn parse_refusal(text: &str) -> Option<String> {
    let rest = text.trim().strip_prefix("not claimed ")?;
    let (_, why) = rest.split_once(": ")?;
    Some(why.trim().to_string())
}
