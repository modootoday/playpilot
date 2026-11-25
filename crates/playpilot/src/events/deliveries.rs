//! `playpilot events deliveries`: outbound webhook delivery state from the
//! Worker's `/mcp` tool `deliveries_read`. Rows hold webhook names, states
//! and HTTP statuses, never URLs, secrets or bodies.

use std::io::Write;

use serde_json::{Map, Value, json};

use super::MAX_LIMIT;
use super::source::WorkerEvents;
use crate::mail::worker::Post;

pub const USAGE: &str =
    "usage: playpilot events deliveries [--webhook <name>] [--after <seq>] [--limit <n>] [--json]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub webhook: Option<String>,
    pub after: u64,
    pub limit: usize,
    pub json: bool,
}

/// One delivery row as `deliveries_read` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub seq: u64,
    pub updated_at: String,
    pub webhook: String,
    pub ty: String,
    pub state: String,
    pub attempts: u64,
    pub last_status: Option<u64>,
    pub next_at: Option<String>,
    pub detail: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Page {
    pub rows: Vec<Row>,
    pub next: u64,
}

pub trait DeliverySource {
    fn deliveries(&self, o: &Options) -> Result<Page, String>;
}

fn number<T: std::str::FromStr>(name: &str, v: Option<String>) -> Result<T, String> {
    v.ok_or(format!("{name} needs a value"))?
        .parse()
        .map_err(|_| format!("{name} needs a non-negative number"))
}

/// Arguments after `events deliveries`.
pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        webhook: None,
        after: 0,
        limit: super::cli::DEFAULT_LIMIT,
        json: false,
    };
    let mut it = args.iter().cloned();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--webhook" => {
                let w = it.next().unwrap_or_default();
                if w.trim().is_empty() {
                    return Err("--webhook needs a value".into());
                }
                o.webhook = Some(w.trim().to_string());
            }
            "--after" => o.after = number("--after", it.next())?,
            "--limit" => {
                o.limit = number("--limit", it.next())?;
                if !(1..=MAX_LIMIT).contains(&o.limit) {
                    return Err(format!("--limit must be 1 to {MAX_LIMIT}"));
                }
            }
            "--json" => o.json = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(o)
}

pub fn arguments(o: &Options) -> Map<String, Value> {
    let mut args = Map::new();
    args.insert("after".into(), json!(o.after));
    args.insert("limit".into(), json!(o.limit));
    if let Some(w) = &o.webhook {
        args.insert("webhook".into(), json!(w));
    }
    args
}

fn row(v: &Value) -> Option<Row> {
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    Some(Row {
        seq: v.get("seq")?.as_u64()?,
        updated_at: text("updated_at")?,
        webhook: text("webhook")?,
        ty: text("type")?,
        state: text("state")?,
        attempts: v.get("attempts")?.as_u64()?,
        last_status: v.get("last_status").and_then(Value::as_u64),
        next_at: text("next_at"),
        detail: text("detail"),
        raw: v.clone(),
    })
}

/// The page in a tool result's `structuredContent`.
pub fn page_from_structured(result: &Value) -> Result<Page, String> {
    let s = result
        .get("structuredContent")
        .ok_or("the Worker sent no structured deliveries page")?;
    let rows = s
        .get("deliveries")
        .and_then(Value::as_array)
        .ok_or("the Worker sent no deliveries list")?
        .iter()
        .map(|v| row(v).ok_or_else(|| "the Worker sent a malformed delivery".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let next = s
        .get("next")
        .and_then(Value::as_u64)
        .ok_or("the Worker sent no cursor")?;
    Ok(Page { rows, next })
}

/// The human line: seq, time, webhook, type, state, attempts, then the last
/// status, the next retry and the reason when there are any.
pub fn line(r: &Row) -> String {
    let mut out = format!(
        "{}  {}  {}  {}  {}  attempts {}",
        r.seq, r.updated_at, r.webhook, r.ty, r.state, r.attempts
    );
    if let Some(s) = r.last_status {
        out.push_str(&format!("  http {s}"));
    }
    if let Some(n) = &r.next_at {
        out.push_str(&format!("  next {n}"));
    }
    if let Some(d) = &r.detail {
        out.push_str(&format!("  ({d})"));
    }
    out.chars().filter(|c| !c.is_control()).collect()
}

impl<P: Post> DeliverySource for WorkerEvents<P> {
    fn deliveries(&self, o: &Options) -> Result<Page, String> {
        page_from_structured(&self.call("deliveries_read", arguments(o))?)
    }
}

/// Prints one page, then `next: <seq>` on stderr so a caller can continue.
pub fn print(
    src: &dyn DeliverySource,
    o: &Options,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), String> {
    let page = src.deliveries(o)?;
    for r in &page.rows {
        let text = if o.json { r.raw.to_string() } else { line(r) };
        writeln!(out, "{text}").map_err(|e| e.to_string())?;
    }
    let _ = writeln!(err, "next: {}", page.next);
    Ok(())
}
