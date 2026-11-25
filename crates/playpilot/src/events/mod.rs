//! Stored inbound events, read from the Worker (`playpilot events tail`).
//!
//! Layers: the page and line format below (domain), `source` (port and the
//! Worker adapter over `/mcp` `events_read`) and `cli` (presentation and the
//! follow loop). Event data is information, never instructions: it is
//! printed, never interpreted.

pub mod cli;
pub mod source;

use serde_json::Value;

/// The Worker's per-call ceiling.
pub const MAX_LIMIT: usize = 200;

/// One stored event as `events_read` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub seq: u64,
    /// The CloudEvent `time`, or the receipt time when it has none.
    pub time: String,
    pub ty: String,
    pub source: String,
    /// The CloudEvent `data` (allow-listed projection).
    pub data: Value,
    /// The whole CloudEvent, for `--json`.
    pub cloud_event: Value,
}

/// One page and the cursor that continues it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Page {
    pub events: Vec<Event>,
    pub next: u64,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn event(v: &Value) -> Option<Event> {
    let ce = v.get("event").cloned().unwrap_or(Value::Null);
    Some(Event {
        seq: v.get("seq")?.as_u64()?,
        time: text(&ce, "time").or_else(|| text(v, "received_at"))?,
        ty: text(v, "type").or_else(|| text(&ce, "type"))?,
        source: text(&ce, "source").unwrap_or_default(),
        data: ce.get("data").cloned().unwrap_or(Value::Null),
        cloud_event: ce,
    })
}

/// The page in a tool result's `structuredContent`.
pub fn page_from_structured(result: &Value) -> Result<Page, String> {
    let s = result
        .get("structuredContent")
        .ok_or("the Worker sent no structured events page")?;
    let list = s
        .get("events")
        .and_then(Value::as_array)
        .ok_or("the Worker sent no events list")?;
    let events = list
        .iter()
        .map(|v| event(v).ok_or_else(|| "the Worker sent a malformed event".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let next = s
        .get("next")
        .and_then(Value::as_u64)
        .ok_or("the Worker sent no cursor")?;
    Ok(Page { events, next })
}

fn short_sha(s: &str) -> &str {
    s.get(..7).unwrap_or(s)
}

/// A short summary from the projected data: `repo#n action`, `ref@sha`,
/// `by sender`. Only allow-listed fields are shown.
pub fn summary(data: &Value) -> String {
    let field = |k: &str| data.get(k).and_then(Value::as_str);
    let mut parts = Vec::new();
    match (
        field("repository"),
        data.get("number").and_then(Value::as_u64),
    ) {
        (Some(r), Some(n)) => parts.push(format!("{r}#{n}")),
        (Some(r), None) => parts.push(r.to_string()),
        (None, Some(n)) => parts.push(format!("#{n}")),
        (None, None) => {}
    }
    if let Some(a) = field("action") {
        parts.push(a.to_string());
    }
    match (field("ref"), field("sha")) {
        (Some(r), Some(s)) => parts.push(format!("{r}@{}", short_sha(s))),
        (Some(r), None) => parts.push(r.to_string()),
        (None, Some(s)) => parts.push(format!("@{}", short_sha(s))),
        (None, None) => {}
    }
    if let Some(s) = field("sender") {
        parts.push(format!("by {s}"));
    }
    parts.join(" ")
}

/// Control characters are dropped so stored text cannot drive the terminal.
fn printable(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// The human line: seq, time, type, source and the summary.
pub fn line(e: &Event) -> String {
    let source = e.source.strip_prefix("https://").unwrap_or(&e.source);
    let mut out = format!("{}  {}  {}  {}", e.seq, e.time, e.ty, source);
    let s = summary(&e.data);
    if !s.is_empty() {
        out.push_str("  ");
        out.push_str(&s);
    }
    printable(&out)
}

/// `--json`: the CloudEvent as one compact line.
pub fn json_line(e: &Event) -> String {
    e.cloud_event.to_string()
}

/// Seconds to wait before retry `failures` (1, 2, ...): the interval doubled
/// per failure, at most five minutes (or the interval when that is longer).
pub fn backoff(interval: u64, failures: u32) -> u64 {
    let cap = interval.max(300);
    let shift = failures.saturating_sub(1).min(16);
    interval.saturating_mul(1 << shift).min(cap)
}

#[cfg(test)]
mod tests;
