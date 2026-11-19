//! `NodeObject`: heartbeat bookkeeping for every node.

use worker::{DurableObject, Env, Request, Response, Result, State, durable_object};

use super::{command, doc, reply};
use crate::domain::node::{self, NodeBook, NodeCmd};
use crate::infra::settings::now;

#[durable_object]
pub struct NodeObject {
    state: State,
}

impl DurableObject for NodeObject {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let cmd: NodeCmd = command(req).await?;
        let sql = self.state.storage().sql();
        doc::init(&sql)?;
        let mut book: NodeBook = doc::load(&sql)?;
        let out = node::apply(&mut book, cmd, now());
        doc::save(&sql, &book)?;
        reply(&out)
    }
}
