//! The `deliveries_read` tool: a cursor read of outbound webhook deliveries
//! (`EventsObject`). Rows carry webhook names, states and HTTP statuses,
//! never URLs, secrets or bodies, so any valid `/mcp` credential may read.

use serde_json::{Value, json};

use super::events::{DEFAULT_LIMIT, uint};
use crate::domain::eventlog::{LogCmd, MAX_READ};
use crate::domain::outbound::{Delivery, State};
use crate::domain::time::rfc3339;

pub const NAME: &str = "deliveries_read";
pub const DESCRIPTION: &str = "Read outbound webhook deliveries (state, attempts, last HTTP status, next retry), oldest first, after a cursor. Pass the returned `next` as `after` to continue.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveriesRead {
    pub after: u64,
    pub limit: usize,
    pub webhook: Option<String>,
}

pub fn properties() -> Value {
    json!({
        "after": {"type": "integer", "minimum": 0, "description": "return deliveries with seq greater than this (default 0)"},
        "limit": {"type": "integer", "minimum": 1, "description": "most deliveries to return (default 100; more than 200 is capped)"},
        "webhook": {"type": "string", "description": "only this Webhook resource (metadata.name)"}
    })
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 100
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

pub fn parse(args: &Value) -> Result<DeliveriesRead, String> {
    let after = uint(args, "after")?.unwrap_or(0);
    let limit = match uint(args, "limit")? {
        None => DEFAULT_LIMIT,
        Some(0) => return Err("`limit` must be at least 1".into()),
        Some(n) => usize::try_from(n).unwrap_or(MAX_READ).min(MAX_READ),
    };
    let webhook = match args.get("webhook") {
        None | Some(Value::Null) => None,
        Some(Value::String(w)) if w.trim().is_empty() => None,
        Some(Value::String(w)) if valid_name(w.trim()) => Some(w.trim().to_string()),
        Some(_) => return Err("`webhook` must be a Webhook resource name".into()),
    };
    Ok(DeliveriesRead {
        after,
        limit,
        webhook,
    })
}

pub fn command(r: &DeliveriesRead) -> LogCmd {
    LogCmd::Deliveries {
        after: r.after,
        limit: r.limit,
        webhook: r.webhook.clone(),
    }
}

pub fn next(after: u64, list: &[Delivery]) -> u64 {
    list.last().map_or(after, |d| d.seq)
}

fn state(s: State) -> &'static str {
    match s {
        State::Pending => "pending",
        State::Delivered => "delivered",
        State::Failed => "failed",
    }
}

pub fn render(after: u64, list: &[Delivery]) -> String {
    if list.is_empty() {
        return format!("deliveries: nothing after {after}");
    }
    let mut lines: Vec<String> = list
        .iter()
        .map(|d| {
            format!(
                "{}  {}  {}  {}  {}  attempts {}",
                d.seq,
                rfc3339(d.updated_at),
                d.webhook,
                d.ty,
                state(d.state),
                d.attempts
            )
        })
        .collect();
    lines.push(format!("next: {}", next(after, list)));
    lines.join("\n")
}

pub fn structured(after: u64, list: &[Delivery]) -> Value {
    let rows: Vec<Value> = list
        .iter()
        .map(|d| {
            json!({
                "seq": d.seq,
                "id": d.id,
                "webhook": d.webhook,
                "event_id": d.event_id,
                "type": d.ty,
                "state": state(d.state),
                "attempts": d.attempts,
                "last_status": d.last_status,
                "detail": d.detail,
                "next_at": (d.state == State::Pending).then(|| rfc3339(d.next_at)),
                "created_at": rfc3339(d.created_at),
                "updated_at": rfc3339(d.updated_at),
            })
        })
        .collect();
    json!({"deliveries": rows, "next": next(after, list)})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::mcp::tools::{self, Command};

    #[test]
    fn parses_arguments() {
        let r = parse(&json!({"after": "2", "limit": 900, "webhook": " owner-notify "})).unwrap();
        assert_eq!(
            r,
            DeliveriesRead {
                after: 2,
                limit: MAX_READ,
                webhook: Some("owner-notify".into())
            }
        );
        assert!(parse(&json!({"limit": 0})).is_err());
        assert!(parse(&json!({"webhook": "a b"})).is_err());
        assert_eq!(parse(&json!({})).unwrap().limit, DEFAULT_LIMIT);
        let c = tools::parse("deliveries_read", &json!({"webhook": "w"})).unwrap();
        assert!(matches!(c.command, Command::DeliveriesRead(_)));
        assert!(tools::parse("deliveries_read", &json!({"url": "x"})).is_err());
    }

    #[test]
    fn renders_state_without_bodies_or_urls() {
        let mut d = Delivery::new("w", "d-1", "com.github.push", "{\"secret\":1}", 0);
        d.seq = 7;
        d.attempts = 1;
        d.next_at = 30;
        d.last_status = Some(503);
        let v = structured(0, &[d.clone()]);
        assert_eq!(v["next"], 7);
        let row = &v["deliveries"][0];
        assert_eq!(row["state"], "pending");
        assert_eq!(row["next_at"], "1970-01-01T00:00:30Z");
        assert_eq!(row["last_status"], 503);
        assert!(!v.to_string().contains("secret"));
        assert!(render(0, &[d]).contains("w  com.github.push  pending  attempts 1"));
        assert_eq!(render(7, &[]), "deliveries: nothing after 7");
    }
}
