use std::cell::RefCell;

use serde_json::{Value, json};

use super::*;
use crate::mail::{Backend, inbox, send};

/// Replays canned `(status, body)` replies and records each request.
#[derive(Default)]
struct Script {
    replies: RefCell<Vec<(u16, String)>>,
    seen: RefCell<Vec<Value>>,
}

impl Script {
    fn new(replies: &[(u16, Value)]) -> Self {
        Self {
            replies: RefCell::new(
                replies
                    .iter()
                    .rev()
                    .map(|(s, v)| (*s, v.to_string()))
                    .collect(),
            ),
            seen: RefCell::default(),
        }
    }
}

impl Post for &Script {
    fn post(&self, body: &str) -> Result<(u16, String), String> {
        self.seen
            .borrow_mut()
            .push(serde_json::from_str(body).unwrap());
        self.replies
            .borrow_mut()
            .pop()
            .ok_or_else(|| "no reply".to_string())
    }
}

fn text(t: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "result": {"content": [{"type": "text", "text": t}], "isError": false}})
}

fn envelope(from: &str, to: &str) -> Envelope {
    Envelope {
        from: from.into(),
        to: to.into(),
        subject: "s".into(),
        re: None,
        body: "b".into(),
    }
}

#[test]
fn send_names_the_actor_and_parses_copies() {
    let script = Script::new(&[(200, text("sent 4 to design"))]);
    let w = Worker::new(&script, Some("ops".into()));
    assert_eq!(
        send(&w, envelope("ops", "design")).unwrap(),
        [("design".to_string(), 4)]
    );
    let req = &script.seen.borrow()[0];
    assert_eq!(req["method"], "tools/call");
    assert_eq!(req["params"]["name"], "mail_send");
    assert_eq!(req["params"]["arguments"]["as"], "ops");
    assert_eq!(req["params"]["arguments"]["to"], "design");
    assert!(req["params"]["arguments"].get("re").is_none());
}

#[test]
fn broadcast_is_one_call_to_all() {
    let script = Script::new(&[(200, text("sent 5 to a\nsent 6 to b"))]);
    let w = Worker::new(&script, None);
    let copies = send(&w, envelope("ops", "all")).unwrap();
    assert_eq!(copies, [("a".to_string(), 5), ("b".to_string(), 6)]);
    assert_eq!(script.seen.borrow().len(), 1);
    assert_eq!(script.seen.borrow()[0]["params"]["arguments"]["to"], "all");
}

#[test]
fn inbox_reads_structured_content_and_skips_the_legacy_box() {
    let mut reply = text("id 7 ...");
    reply["result"]["structuredContent"] = json!({"messages": [
        {"id": 7, "created_at": "2026-11-19T00:00:00Z", "from": "a", "to": "ops",
         "subject": "s", "re": null, "body": "x", "read": false, "acked": false},
        {"id": 8, "created_at": "2026-11-18T00:00:00Z", "from": "a", "to": "ops",
         "subject": "t", "re": "o/r#1", "body": "y", "read": true, "acked": true}
    ]});
    let script = Script::new(&[(200, reply)]);
    let w = Worker::new(&script, Some("ops".into()));
    let got = inbox(&w, "ops", false, "2000-01-01T00:00:00Z").unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].id, 7);
    let req = &script.seen.borrow()[0];
    assert_eq!(req["params"]["name"], "mail_inbox");
    assert_eq!(
        req["params"]["arguments"],
        json!({"all": true, "as": "ops"})
    );
}

#[test]
fn receipts_carry_the_actor() {
    let script = Script::new(&[(200, text("ack 3"))]);
    let w = Worker::new(&script, Some("ops".into()));
    w.receipt(3, Receipt::Ack).unwrap();
    let req = &script.seen.borrow()[0];
    assert_eq!(req["params"]["name"], "mail_ack");
    assert_eq!(req["params"]["arguments"], json!({"id": 3, "as": "ops"}));
}

#[test]
fn errors_surface_as_text() {
    let tool_err = json!({"jsonrpc": "2.0", "id": 1, "result": {"content": [{"type": "text", "text": "`as` is required"}], "isError": true}});
    assert_eq!(
        tool_outcome(200, &tool_err.to_string()).unwrap_err(),
        "`as` is required"
    );
    let rpc_err = json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32602, "message": "bad"}});
    assert!(
        tool_outcome(200, &rpc_err.to_string())
            .unwrap_err()
            .contains("bad")
    );
    assert!(
        tool_outcome(401, "")
            .unwrap_err()
            .contains("PLAYPILOT_MCP_TOKEN")
    );
    assert!(tool_outcome(502, "").unwrap_err().contains("502"));
    assert!(tool_outcome(200, "<html>").is_err());
}

#[test]
fn curl_config_quotes_values_and_keeps_the_token_off_argv() {
    let c = curl_config(
        "https://mail.example.invalid/mcp",
        Some("tok"),
        r#"{"a":"q\"x\\y"}"#,
    );
    assert!(c.contains("url = \"https://mail.example.invalid/mcp\"\n"));
    assert!(c.contains("header = \"Authorization: Bearer tok\"\n"));
    assert!(c.contains(r#"data-binary = "{\"a\":\"q\\\"x\\\\y\"}""#));
    assert!(c.contains("write-out = \"\\n%{http_code}\""));
    assert!(!curl_config("u", None, "{}").contains("Authorization"));
    assert!(!curl_config("u", Some(" "), "{}").contains("Authorization"));
    assert_eq!(
        split_status("{\"x\":1}\n200").unwrap(),
        (200, "{\"x\":1}".into())
    );
    assert_eq!(split_status("401").unwrap(), (401, String::new()));
    assert!(split_status("oops").is_err());
}

#[test]
fn parses_sent_lines() {
    assert_eq!(
        parse_sent("sent 1 to a\nnoise\nsent x to b\nsent 2 to b"),
        [("a".to_string(), 1), ("b".to_string(), 2)]
    );
}

#[test]
fn the_worker_url_selects_the_worker() {
    let url = Some("https://mail.example.invalid/mcp".to_string());
    let repo = Some("o/r".to_string());
    assert_eq!(
        Backend::choose(url.clone(), repo.clone()),
        Some(Backend::Worker {
            url: url.clone().unwrap()
        })
    );
    assert_eq!(
        Backend::choose(Some(" ".into()), repo.clone()),
        Some(Backend::GitHub { repo: "o/r".into() })
    );
    assert_eq!(Backend::choose(None, None), None);
    assert_eq!(
        Backend::choose(url, None).unwrap().to_string(),
        "worker https://mail.example.invalid/mcp"
    );
}
