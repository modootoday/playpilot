//! `EventsObject`: the inbound event log (GitHub deliveries).

use worker::{DurableObject, Env, Request, Response, Result, State, durable_object};

use super::sql_events::SqlEvents;
use super::{command, reply};
use crate::domain::eventlog::{self, LogCmd};
use crate::infra::settings::now;

#[durable_object]
pub struct EventsObject {
    state: State,
}

impl DurableObject for EventsObject {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let cmd: LogCmd = command(req).await?;
        let store = SqlEvents::open(self.state.storage().sql())?;
        reply(&eventlog::apply(&store, cmd, now()))
    }
}
