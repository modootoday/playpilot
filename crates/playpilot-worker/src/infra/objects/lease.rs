//! `LeaseObject`: the single owner of every lane-item lease.

use worker::{DurableObject, Env, Request, Response, Result, State, durable_object};

use super::{command, doc, reply};
use crate::domain::lease::{self, LeaseBook, LeaseCmd};
use crate::infra::settings::now;

#[durable_object]
pub struct LeaseObject {
    state: State,
}

impl DurableObject for LeaseObject {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let cmd: LeaseCmd = command(req).await?;
        let sql = self.state.storage().sql();
        doc::init(&sql)?;
        let mut book: LeaseBook = doc::load(&sql)?;
        let out = lease::apply(&mut book, cmd, now());
        doc::save(&sql, &book)?;
        reply(&out)
    }
}
