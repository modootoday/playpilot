//! `BusObject`: mailboxes and the event log.

use worker::{DurableObject, Env, Request, Response, Result, State, durable_object};

use super::sql_bus::SqlBus;
use super::{command, reply};
use crate::domain::bus::{self, BusCmd};
use crate::infra::settings::now;

#[durable_object]
pub struct BusObject {
    state: State,
}

impl DurableObject for BusObject {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let cmd: BusCmd = command(req).await?;
        let store = SqlBus::open(self.state.storage().sql())?;
        reply(&bus::apply(&store, cmd, now()))
    }
}
