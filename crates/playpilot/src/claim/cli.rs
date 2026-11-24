//! Presentation: `playpilot claim` and `playpilot release`.

use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use super::github::GitHubTracker;
use super::worker::WorkerLeases;
use super::{
    Outcome, claim, claim_leased, release, release_expired, release_leased, sweep_expired,
};
use crate::mail::worker::Curl;

pub const USAGE: &str = "\
usage: playpilot claim   <owner/repo#n> [--as <id>] [--stale-hours <n>] [--backend github|worker] [--lane <name>]
       playpilot release <owner/repo#n> [--as <id>] [--note <text>] [--backend github|worker]
       playpilot release --expired <owner/repo#n | owner/repo>
id: --as, or PLAYPILOT_CLAIM_ID, or PLAYPILOT_ACTOR. Use an id that names no device.
backend: --backend, or PLAYPILOT_CLAIM_BACKEND (default github). worker takes a lease on the
         Worker at PLAYPILOT_MAIL_URL first (TTL --stale-hours, 1 to 24) and mirrors it on the item.
--expired: ends claims whose mirrored lease has run out, on one item or every `claimed` item.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    GitHub,
    Worker,
}

pub fn parse_backend(v: &str) -> Result<Backend, String> {
    match v {
        "github" => Ok(Backend::GitHub),
        "worker" => Ok(Backend::Worker),
        _ => Err(format!("backend `{v}` is not github or worker")),
    }
}

fn worker_leases() -> Result<WorkerLeases, String> {
    let url = std::env::var("PLAYPILOT_MAIL_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .ok_or("the worker backend needs PLAYPILOT_MAIL_URL")?;
    let actor = std::env::var("PLAYPILOT_ACTOR")
        .ok()
        .filter(|a| !a.is_empty());
    Ok(WorkerLeases::new(
        Curl {
            url,
            token: std::env::var("PLAYPILOT_MCP_TOKEN").ok(),
        },
        actor,
    ))
}

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
        Outcome::Refused(why) => (format!("{item}: not claimed: {why}"), false),
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
    let mut it = args.into_iter().peekable();
    if verb == "release" && it.peek().map(String::as_str) == Some("--expired") {
        it.next();
        let target = it.next().ok_or(USAGE)?;
        if it.next().is_some() {
            return Err(USAGE.into());
        }
        return expired(&target);
    }
    let item = it.next().filter(|a| !a.starts_with('-')).ok_or(USAGE)?;
    let (mut id, mut stale, mut note) = (env_id(), 6u64, None);
    let mut lane = "default".to_string();
    let mut backend = match std::env::var("PLAYPILOT_CLAIM_BACKEND") {
        Ok(v) if !v.is_empty() => parse_backend(&v)?,
        _ => Backend::GitHub,
    };
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
            "--backend" => backend = parse_backend(&value()?)?,
            "--lane" => lane = value()?,
            _ => return Err(USAGE.into()),
        }
    }
    let id = id.ok_or("set --as, PLAYPILOT_CLAIM_ID or PLAYPILOT_ACTOR")?;
    let t = GitHubTracker;
    match (verb, backend) {
        ("claim", Backend::GitHub) => {
            let (text, ok) = describe(&item, &claim(&t, &item, &id, &now_utc(), stale)?);
            println!("{text}");
            Ok(ok)
        }
        ("claim", Backend::Worker) => {
            crate::claim::tracker::parse_item(&item)?;
            let l = worker_leases()?;
            let o = claim_leased(&t, &l, &item, &id, &lane, &now_utc(), stale)?;
            let (text, ok) = describe(&item, &o);
            println!("{text}");
            Ok(ok)
        }
        (_, Backend::GitHub) => {
            release(&t, &item, &id, note.as_deref())?;
            println!("released {item}");
            Ok(true)
        }
        (_, Backend::Worker) => {
            crate::claim::tracker::parse_item(&item)?;
            release_leased(&t, &worker_leases()?, &item, &id, note.as_deref())?;
            println!("released {item} and its lease");
            Ok(true)
        }
    }
}

/// `release --expired`: one item, or every `claimed` item in a repository.
fn expired(target: &str) -> Result<bool, String> {
    let t = GitHubTracker;
    let now = now_utc();
    if target.contains('#') {
        crate::claim::tracker::parse_item(target)?;
        match release_expired(&t, target, &now)? {
            Some(who) => println!("released {target} (expired lease of {who})"),
            None => println!("{target}: no expired lease"),
        }
        return Ok(true);
    }
    let done = sweep_expired(&t, target, &now)?;
    if done.is_empty() {
        println!("{target}: no expired leases");
    }
    for (item, who) in done {
        println!("released {item} (expired lease of {who})");
    }
    Ok(true)
}
