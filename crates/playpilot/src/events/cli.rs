//! Presentation: `playpilot events tail`.

use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use super::source::{EventSource, Query, WorkerEvents};
use super::{MAX_LIMIT, backoff, json_line, line};
use crate::mail::worker::Curl;

pub const USAGE: &str = "\
usage: playpilot events tail [--type <prefix>] [--after <seq>] [--limit <n>] [--follow [--interval <secs>]] [--json]
source: the Worker at PLAYPILOT_MAIL_URL (token from PLAYPILOT_MCP_TOKEN, if set)";

pub const DEFAULT_LIMIT: usize = 100;
pub const DEFAULT_INTERVAL: u64 = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub ty: Option<String>,
    pub after: u64,
    pub limit: usize,
    pub follow: bool,
    pub interval: u64,
    pub json: bool,
}

fn number<T: std::str::FromStr>(name: &str, v: Option<String>) -> Result<T, String> {
    v.ok_or(format!("{name} needs a value"))?
        .parse()
        .map_err(|_| format!("{name} needs a non-negative number"))
}

/// Arguments after `events`. Errors are usage errors.
pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut it = args.iter().cloned();
    match it.next().as_deref() {
        Some("tail") => {}
        Some(v) => return Err(format!("unknown events verb `{v}`")),
        None => return Err("missing verb".into()),
    }
    let mut o = Options {
        ty: None,
        after: 0,
        limit: DEFAULT_LIMIT,
        follow: false,
        interval: DEFAULT_INTERVAL,
        json: false,
    };
    let mut interval_given = false;
    while let Some(x) = it.next() {
        match x.as_str() {
            "--type" => {
                let t = it.next().ok_or("--type needs a value")?;
                if t.trim().is_empty() {
                    return Err("--type needs a value".into());
                }
                o.ty = Some(t.trim().to_string());
            }
            "--after" => o.after = number("--after", it.next())?,
            "--limit" => {
                o.limit = number("--limit", it.next())?;
                if !(1..=MAX_LIMIT).contains(&o.limit) {
                    return Err(format!("--limit must be 1 to {MAX_LIMIT}"));
                }
            }
            "--interval" => {
                o.interval = number("--interval", it.next())?;
                if o.interval == 0 {
                    return Err("--interval must be at least 1".into());
                }
                interval_given = true;
            }
            "--follow" | "-f" => o.follow = true,
            "--json" => o.json = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if interval_given && !o.follow {
        return Err("--interval needs --follow".into());
    }
    Ok(o)
}

/// Prints pages from `src`. Without `follow` one read, whose error is
/// returned. With `follow` it polls with the cursor: a full page is read
/// again at once, otherwise `sleep(interval)`; an error is printed to `err`
/// and retried after `sleep(backoff)`. It returns when `sleep` returns
/// false or `out` is closed.
pub fn tail(
    src: &dyn EventSource,
    o: &Options,
    out: &mut dyn Write,
    err: &mut dyn Write,
    sleep: &mut dyn FnMut(u64) -> bool,
) -> Result<(), String> {
    let mut after = o.after;
    let mut failures = 0u32;
    loop {
        let q = Query {
            after,
            limit: o.limit,
            ty: o.ty.clone(),
        };
        let wait = match src.read(&q) {
            Ok(page) => {
                failures = 0;
                for e in &page.events {
                    let text = if o.json { json_line(e) } else { line(e) };
                    if writeln!(out, "{text}").and_then(|_| out.flush()).is_err() {
                        return Ok(());
                    }
                }
                let advanced = page.next > after;
                after = page.next.max(after);
                if !o.follow {
                    return Ok(());
                }
                if advanced && page.events.len() >= o.limit {
                    continue;
                }
                o.interval
            }
            Err(e) if !o.follow => return Err(e),
            Err(e) => {
                failures = failures.saturating_add(1);
                let wait = backoff(o.interval, failures);
                let _ = writeln!(err, "playpilot events: {e} (retry in {wait}s)");
                wait
            }
        };
        if !sleep(wait) {
            return Ok(());
        }
    }
}

fn worker() -> Result<WorkerEvents, String> {
    let url = std::env::var("PLAYPILOT_MAIL_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .ok_or("events need the Worker: set PLAYPILOT_MAIL_URL")?;
    Ok(WorkerEvents::new(Curl {
        url,
        token: std::env::var("PLAYPILOT_MCP_TOKEN").ok(),
    }))
}

pub fn main(args: Vec<String>) -> ExitCode {
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    }
    let o = match parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("playpilot events: {e}\n{USAGE}");
            return ExitCode::from(64);
        }
    };
    let run = worker().and_then(|src| {
        let mut sleep = |secs: u64| {
            std::thread::sleep(Duration::from_secs(secs));
            true
        };
        tail(
            &src,
            &o,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
            &mut sleep,
        )
    });
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("playpilot events: {e}");
            ExitCode::from(1)
        }
    }
}
