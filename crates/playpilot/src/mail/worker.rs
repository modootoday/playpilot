//! Infrastructure: mailboxes on the playpilot Worker, through its `/mcp`
//! JSON-RPC endpoint, by running `curl` (no HTTP library in this binary).
//!
//! The Worker authenticates `/mcp` with a bearer token. It comes from
//! `PLAYPILOT_MCP_TOKEN` when set; otherwise none is sent, for cloud sessions
//! whose network proxy injects the credential for the Worker's host. One
//! token may stand for several actors, so every call names its actor (`as`).

use std::cell::Cell;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Map, Value, json};

use super::BROADCAST;
use super::message::{Envelope, Stored};
use super::transport::{Receipt, Transport};

/// Sends one JSON-RPC body, returns the HTTP status and response body.
pub trait Post {
    fn post(&self, body: &str) -> Result<(u16, String), String>;
}

/// `curl` with its options on stdin, so the token never appears in a
/// process listing.
pub struct Curl {
    pub url: String,
    pub token: Option<String>,
}

pub struct Worker<P: Post = Curl> {
    pub post: P,
    /// This session's actor (`PLAYPILOT_ACTOR`), sent as `as` on receipts.
    pub actor: Option<String>,
    next_id: Cell<u64>,
}

impl<P: Post> Worker<P> {
    pub fn new(post: P, actor: Option<String>) -> Self {
        Self {
            post,
            actor,
            next_id: Cell::new(1),
        }
    }

    /// Calls one tool and returns its JSON-RPC `result`.
    fn call(&self, tool: &str, args: Map<String, Value>) -> Result<Value, String> {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let (status, text) = self.post.post(&rpc_request(id, tool, args))?;
        tool_outcome(status, &text)
    }
}

/// Quotes a value for a curl config file.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The curl config (`curl --config -`) for one POST.
pub fn curl_config(url: &str, token: Option<&str>, body: &str) -> String {
    let mut lines = vec![
        format!("url = {}", quote(url)),
        "request = \"POST\"".to_string(),
        "header = \"Content-Type: application/json\"".to_string(),
        "header = \"Accept: application/json\"".to_string(),
        "max-time = 60".to_string(),
        "write-out = \"\\n%{http_code}\"".to_string(),
    ];
    if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
        lines.push(format!(
            "header = {}",
            quote(&format!("Authorization: Bearer {}", t.trim()))
        ));
    }
    lines.push(format!("data-binary = {}", quote(body)));
    lines.join("\n") + "\n"
}

/// Splits curl's output into the response body and the status `write-out` appended.
pub fn split_status(out: &str) -> Result<(u16, String), String> {
    let (body, code) = out.rsplit_once('\n').unwrap_or(("", out));
    let status = code
        .trim()
        .parse()
        .map_err(|_| "curl did not report an HTTP status".to_string())?;
    Ok((status, body.to_string()))
}

impl Post for Curl {
    fn post(&self, body: &str) -> Result<(u16, String), String> {
        let mut child = Command::new("curl")
            .args(["-sS", "--config", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run curl: {e}"))?;
        let config = curl_config(&self.url, self.token.as_deref(), body);
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(config.as_bytes())
                .map_err(|e| format!("could not write to curl: {e}"))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("curl failed: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "curl failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        split_status(&String::from_utf8_lossy(&out.stdout))
    }
}

pub fn rpc_request(id: u64, tool: &str, args: Map<String, Value>) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": tool, "arguments": args}
    })
    .to_string()
}

/// The `result` of a successful tool call; transport, protocol and tool
/// errors become `Err` text.
pub fn tool_outcome(status: u16, text: &str) -> Result<Value, String> {
    match status {
        200 => {}
        401 => {
            return Err("the mail Worker refused the credential (HTTP 401): set PLAYPILOT_MCP_TOKEN, or the environment credential for the Worker host".into());
        }
        s => return Err(format!("the mail Worker answered HTTP {s}")),
    }
    let v: Value =
        serde_json::from_str(text).map_err(|e| format!("the mail Worker sent non-JSON: {e}"))?;
    if let Some(e) = v.get("error") {
        let msg = e.get("message").and_then(Value::as_str).unwrap_or("error");
        return Err(format!("mail Worker: {msg}"));
    }
    let result = v.get("result").cloned().ok_or("mail Worker: no result")?;
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(text_of(&result));
    }
    Ok(result)
}

pub fn text_of(result: &Value) -> String {
    result
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// `sent <id> to <mailbox>` lines, as the Worker renders a send.
pub fn parse_sent(text: &str) -> Vec<(String, u64)> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("sent ")?;
            let (id, mailbox) = rest.split_once(" to ")?;
            Some((mailbox.trim().to_string(), id.trim().parse().ok()?))
        })
        .collect()
}

/// Messages from the Worker inbox's `structuredContent`.
pub fn stored_from_structured(result: &Value) -> Result<Vec<Stored>, String> {
    let list = result
        .pointer("/structuredContent/messages")
        .and_then(Value::as_array)
        .ok_or("the mail Worker sent no structured inbox")?;
    list.iter()
        .map(|m| stored(m).ok_or_else(|| "the mail Worker sent a malformed message".to_string()))
        .collect()
}

fn stored(m: &Value) -> Option<Stored> {
    let s = |k: &str| m.get(k).and_then(Value::as_str).map(str::to_string);
    let b = |k: &str| m.get(k).and_then(Value::as_bool).unwrap_or(false);
    Some(Stored {
        id: m.get("id")?.as_u64()?,
        created_at: s("created_at")?,
        envelope: Envelope {
            from: s("from")?,
            to: s("to")?,
            subject: s("subject")?,
            re: s("re"),
            body: s("body")?,
        },
        read: b("read"),
        acked: b("acked"),
    })
}

fn args(pairs: &[(&str, Option<Value>)]) -> Map<String, Value> {
    pairs
        .iter()
        .filter_map(|(k, v)| v.clone().map(|v| (k.to_string(), v)))
        .collect()
}

impl<P: Post> Worker<P> {
    fn send_to(&self, to: &str, e: &Envelope) -> Result<Vec<(String, u64)>, String> {
        let result = self.call(
            "mail_send",
            args(&[
                ("to", Some(json!(to))),
                ("subject", Some(json!(e.subject))),
                ("body", Some(json!(e.body))),
                ("re", e.re.as_ref().map(|r| json!(r))),
                ("as", Some(json!(e.from))),
            ]),
        )?;
        let sent = parse_sent(&text_of(&result));
        if sent.is_empty() {
            return Err("the mail Worker did not confirm the send".into());
        }
        Ok(sent)
    }
}

impl<P: Post> Transport for Worker<P> {
    fn send(&self, mailbox: &str, envelope: &Envelope) -> Result<u64, String> {
        let sent = self.send_to(mailbox, envelope)?;
        Ok(sent
            .iter()
            .find(|(m, _)| m == mailbox)
            .unwrap_or(&sent[0])
            .1)
    }

    fn broadcast(&self, envelope: &Envelope) -> Option<Result<Vec<(String, u64)>, String>> {
        Some(self.send_to(BROADCAST, envelope))
    }

    /// The Worker keeps its own directory and copies broadcasts itself.
    fn actors(&self) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }

    fn list(&self, actor: &str) -> Result<Vec<Stored>, String> {
        if actor == BROADCAST {
            return Ok(Vec::new());
        }
        let result = self.call(
            "mail_inbox",
            args(&[("all", Some(json!(true))), ("as", Some(json!(actor)))]),
        )?;
        stored_from_structured(&result)
    }

    fn receipt(&self, id: u64, kind: Receipt) -> Result<(), String> {
        let tool = match kind {
            Receipt::Read => "mail_read",
            Receipt::Ack => "mail_ack",
        };
        self.call(
            tool,
            args(&[
                ("id", Some(json!(id))),
                ("as", self.actor.as_ref().map(|a| json!(a))),
            ]),
        )
        .map(|_| ())
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
