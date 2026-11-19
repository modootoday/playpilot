//! Claims: one node works on a lane item at a time (`.agents/rules/claims.md`).
//!
//! Layers: the rules below (domain), `tracker` (port), `github` (adapter) and
//! `cli` (presentation). A claim is a comment whose first line is
//! `claim: <id> <UTC time>`; `release: <id>` or `release: stale claim` ends it.
//! The earliest open claim wins. Ids are opaque so public items name no device.

pub mod cli;
pub mod github;
pub mod tracker;

use tracker::{Comment, Tracker};

/// The one label that marks a claimed item; it names no device.
pub const LABEL: &str = "claimed";

/// Claim ids: lower-case words joined by hyphens, like actor names.
pub fn valid_id(id: &str) -> bool {
    crate::mail::message::valid_actor(id)
}

/// Who holds an item, from its comments (oldest first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub id: String,
    pub since: String,
    /// Time of the newest comment after the claim, or the claim itself.
    pub last_activity: String,
}

fn first_line(body: &str) -> &str {
    body.lines().next().unwrap_or_default().trim()
}

/// Replays claim and release comments; returns the current holder.
pub fn holder(comments: &[Comment]) -> Option<Holder> {
    let mut current: Option<Holder> = None;
    for c in comments {
        let line = first_line(&c.body);
        if let Some(rest) = line.strip_prefix("claim:") {
            let id = rest.split_whitespace().next().unwrap_or_default();
            if current.is_none() && valid_id(id) {
                current = Some(Holder {
                    id: id.to_string(),
                    since: c.created_at.clone(),
                    last_activity: c.created_at.clone(),
                });
                continue;
            }
        } else if let Some(rest) = line.strip_prefix("release:") {
            let who = rest.split_whitespace().next().unwrap_or_default();
            let ends = match &current {
                Some(h) => who == h.id || rest.trim() == "stale claim",
                None => false,
            };
            if ends {
                current = None;
                continue;
            }
        }
        if let Some(h) = current.as_mut() {
            h.last_activity = c.created_at.clone();
        }
    }
    current
}

/// RFC 3339 UTC timestamps compare as text; this one is `hours` before `now`.
fn hours_before(now: &str, hours: u64) -> Option<String> {
    let secs = parse_utc(now)?;
    Some(crate::mail::days_ago(secs.saturating_sub(hours * 3600), 0))
}

/// Seconds since the Unix epoch for `YYYY-MM-DDTHH:MM:SSZ`.
pub fn parse_utc(t: &str) -> Option<u64> {
    let b = t.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[10] != b'T' || b[19] != b'Z' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| t.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// This id now holds the item (`took_over` names a stale holder it released).
    Claimed {
        took_over: Option<String>,
    },
    AlreadyYours,
    /// Someone else holds it and the claim is not stale.
    HeldBy(Holder),
    /// Another claim landed first; ours was withdrawn.
    LostRace(String),
}

/// Claims `item` for `id`. `now` is RFC 3339 UTC; `stale_hours` from the fleet.
pub fn claim(
    t: &dyn Tracker,
    item: &str,
    id: &str,
    now: &str,
    stale_hours: u64,
) -> Result<Outcome, String> {
    if !valid_id(id) {
        return Err(format!("`{id}` is not a valid claim id"));
    }
    let mut took_over = None;
    if let Some(h) = holder(&t.comments(item)?) {
        if h.id == id {
            return Ok(Outcome::AlreadyYours);
        }
        let cutoff = hours_before(now, stale_hours).ok_or("`now` must be RFC 3339 UTC")?;
        if h.last_activity.as_str() > cutoff.as_str() {
            return Ok(Outcome::HeldBy(h));
        }
        t.comment(
            item,
            &format!(
                "release: stale claim\n{} has had no activity since {}.",
                h.id, h.last_activity
            ),
        )?;
        took_over = Some(h.id);
    }
    t.comment(item, &format!("claim: {id} {now}"))?;
    t.add_label(item, LABEL)?;
    match holder(&t.comments(item)?) {
        Some(h) if h.id == id => Ok(Outcome::Claimed { took_over }),
        Some(h) => {
            t.comment(item, &format!("release: {id}\nLost the race to {}.", h.id))?;
            Ok(Outcome::LostRace(h.id))
        }
        None => Err("the claim comment was not found after posting it".into()),
    }
}

/// Releases `item` held by `id`, with an optional result note.
pub fn release(t: &dyn Tracker, item: &str, id: &str, note: Option<&str>) -> Result<(), String> {
    match holder(&t.comments(item)?) {
        Some(h) if h.id == id => {}
        Some(h) => return Err(format!("{item} is held by `{}`, not `{id}`", h.id)),
        None => return Err(format!("{item} has no open claim")),
    }
    let mut body = format!("release: {id}");
    if let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) {
        body.push('\n');
        body.push_str(n);
    }
    t.comment(item, &body)?;
    t.remove_label(item, LABEL)
}
