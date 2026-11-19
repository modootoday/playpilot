//! Presentation: `playpilot claim` and `playpilot release`.

use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use super::github::GitHubTracker;
use super::{Outcome, claim, release};

pub const USAGE: &str = "\
usage: playpilot claim   <owner/repo#n> [--as <id>] [--stale-hours <n>]
       playpilot release <owner/repo#n> [--as <id>] [--note <text>]
id: --as, or PLAYPILOT_CLAIM_ID, or PLAYPILOT_ACTOR. Use an id that names no device.";

pub fn now_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    crate::mail::days_ago(secs, 0)
}

pub fn env_id() -> Option<String> {
    std::env::var("PLAYPILOT_CLAIM_ID")
        .or_else(|_| std::env::var("PLAYPILOT_ACTOR"))
        .ok()
        .filter(|s| !s.is_empty())
}

pub fn describe(item: &str, o: &Outcome) -> (String, bool) {
    match o {
        Outcome::Claimed { took_over: None } => (format!("claimed {item}"), true),
        Outcome::Claimed {
            took_over: Some(old),
        } => (
            format!("claimed {item} (released a stale claim by {old})"),
            true,
        ),
        Outcome::AlreadyYours => (format!("{item} is already yours"), true),
        Outcome::HeldBy(h) => (
            format!(
                "{item} is held by {} since {} (last activity {})",
                h.id, h.since, h.last_activity
            ),
            false,
        ),
        Outcome::LostRace(w) => (format!("{item}: {w} claimed first; withdrew"), false),
    }
}

pub fn main(verb: &str, args: Vec<String>) -> ExitCode {
    match run(verb, args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("{verb}: {e}");
            if e.starts_with("usage") {
                return ExitCode::from(64);
            }
            ExitCode::from(1)
        }
    }
}

fn run(verb: &str, args: Vec<String>) -> Result<bool, String> {
    let mut it = args.into_iter();
    let item = it.next().filter(|a| !a.starts_with('-')).ok_or(USAGE)?;
    let (mut id, mut stale, mut note) = (env_id(), 6u64, None);
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--as" => id = Some(value()?),
            "--stale-hours" => {
                stale = value()?
                    .parse()
                    .map_err(|_| "--stale-hours needs a number")?
            }
            "--note" => note = Some(value()?),
            _ => return Err(USAGE.into()),
        }
    }
    let id = id.ok_or("set --as, PLAYPILOT_CLAIM_ID or PLAYPILOT_ACTOR")?;
    let t = GitHubTracker;
    if verb == "claim" {
        let (text, ok) = describe(&item, &claim(&t, &item, &id, &now_utc(), stale)?);
        println!("{text}");
        Ok(ok)
    } else {
        release(&t, &item, &id, note.as_deref())?;
        println!("released {item}");
        Ok(true)
    }
}
