//! Port and adapter: event pages from the Worker's `/mcp` tool
//! `events_read`, on the endpoint and credential the mailbox uses
//! (`PLAYPILOT_MAIL_URL`, `PLAYPILOT_MCP_TOKEN` or the environment's proxy).

use std::cell::Cell;

use serde_json::{Map, Value, json};

use super::{Page, page_from_structured};
use crate::mail::worker::{Curl, Post, rpc_request, tool_outcome};

/// What one read asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub after: u64,
    pub limit: usize,
    pub ty: Option<String>,
}

pub trait EventSource {
    fn read(&self, q: &Query) -> Result<Page, String>;
}

pub struct WorkerEvents<P: Post = Curl> {
    post: P,
    next_id: Cell<u64>,
}

impl<P: Post> WorkerEvents<P> {
    pub fn new(post: P) -> Self {
        Self {
            post,
            next_id: Cell::new(1),
        }
    }
}

pub fn arguments(q: &Query) -> Map<String, Value> {
    let mut args = Map::new();
    args.insert("after".into(), json!(q.after));
    args.insert("limit".into(), json!(q.limit));
    if let Some(t) = &q.ty {
        args.insert("type".into(), json!(t));
    }
    args
}

impl<P: Post> EventSource for WorkerEvents<P> {
    fn read(&self, q: &Query) -> Result<Page, String> {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let (status, text) = self
            .post
            .post(&rpc_request(id, "events_read", arguments(q)))?;
        let result = tool_outcome(status, &text)?;
        page_from_structured(&result)
    }
}
