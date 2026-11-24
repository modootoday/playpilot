//! Adapter: leases through the Worker's `/mcp` tools `task_claim` and
//! `task_release`, on the same endpoint and credential as the mailbox
//! (`PLAYPILOT_MAIL_URL`, `PLAYPILOT_MCP_TOKEN` or the environment's proxy).

use std::cell::Cell;

use serde_json::{Map, Value, json};

use super::lease::{Acquired, Leases, parse_grant, parse_refusal};
use crate::mail::worker::{Curl, Post, rpc_request, text_of, tool_outcome};

pub struct WorkerLeases<P: Post = Curl> {
    post: P,
    /// Sent as `as` when the credential speaks for several actors.
    actor: Option<String>,
    next_id: Cell<u64>,
}

impl<P: Post> WorkerLeases<P> {
    pub fn new(post: P, actor: Option<String>) -> Self {
        Self {
            post,
            actor,
            next_id: Cell::new(1),
        }
    }

    fn call(&self, tool: &str, mut args: Map<String, Value>) -> Result<String, String> {
        if let Some(a) = &self.actor {
            args.insert("as".into(), json!(a));
        }
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let (status, text) = self.post.post(&rpc_request(id, tool, args))?;
        tool_outcome(status, &text).map(|r| text_of(&r))
    }
}

impl<P: Post> Leases for WorkerLeases<P> {
    fn acquire(&self, item: &str, lane: &str, ttl_hours: u64) -> Result<Acquired, String> {
        let mut args = Map::new();
        args.insert("item".into(), json!(item));
        args.insert("lane".into(), json!(lane));
        args.insert("stale_hours".into(), json!(ttl_hours));
        match self.call("task_claim", args) {
            Ok(text) => parse_grant(&text)
                .map(Acquired::Granted)
                .ok_or_else(|| format!("the Worker sent an unknown claim reply: {text}")),
            Err(e) => parse_refusal(&e).map(Acquired::Refused).ok_or(e),
        }
    }

    fn release(&self, item: &str) -> Result<(), String> {
        let mut args = Map::new();
        args.insert("item".into(), json!(item));
        match self.call("task_release", args) {
            Ok(_) => Ok(()),
            Err(e) if e.contains("no open lease") => Ok(()),
            Err(e) => Err(e),
        }
    }
}
