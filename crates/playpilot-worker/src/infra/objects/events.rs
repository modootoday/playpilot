//! `EventsObject`: the inbound event log (GitHub deliveries) and the
//! outbound webhook deliveries, sent from the Durable Object alarm.

use std::time::Duration;

use worker::wasm_bindgen::JsValue;
use worker::{
    AbortSignal, DurableObject, Env, Fetch, Headers, Method, Request, RequestInit, RequestRedirect,
    Response, Result, State, durable_object,
};

use super::sql_deliveries::SqlDeliveries;
use super::sql_events::SqlEvents;
use super::{command, reply};
use crate::domain::eventlog::{self, LogCmd};
use crate::domain::outbound::store::DeliveryStore;
use crate::domain::outbound::target::targets;
use crate::domain::outbound::{self, BATCH, Bindings, Outcome, Outgoing, TIMEOUT_MS};
use crate::infra::settings::{self, now};

/// When the config cannot be read, try the queue again after this long.
const CONFIG_RETRY: Duration = Duration::from_secs(60);

#[durable_object]
pub struct EventsObject {
    state: State,
    env: Env,
}

fn rust(e: impl std::fmt::Display) -> worker::Error {
    worker::Error::RustError(e.to_string())
}

impl EventsObject {
    /// Arms the alarm for the earliest pending delivery, if any.
    async fn arm(&self, outbox: &SqlDeliveries) -> Result<()> {
        let storage = self.state.storage();
        match outbox.next_due().map_err(rust)? {
            Some(at) => {
                let wait = at.saturating_sub(now());
                storage.set_alarm(Duration::from_secs(wait)).await
            }
            None => Ok(()),
        }
    }
}

/// One signed POST with a 10 s timeout; redirects are not followed.
async fn send(r: &Outgoing) -> Outcome {
    let attempt = async {
        let headers = Headers::new();
        for (k, v) in &r.headers {
            headers.set(k, v)?;
        }
        headers.set("user-agent", "playpilot-worker")?;
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_redirect(RequestRedirect::Manual)
            .with_body(Some(JsValue::from_str(&r.body)));
        let req = Request::new_with_init(&r.url, &init)?;
        let signal = AbortSignal::from(worker::web_sys::AbortSignal::timeout_with_u32(TIMEOUT_MS));
        let result = Fetch::Request(req).send_with_signal(&signal).await;
        Ok::<_, worker::Error>((result, signal.aborted()))
    };
    match attempt.await {
        Ok((Ok(resp), _)) => Outcome::Status(resp.status_code()),
        Ok((Err(_), true)) => Outcome::Transport(format!("timed out after {TIMEOUT_MS} ms")),
        Ok((Err(e), false)) | Err(e) => Outcome::Transport(format!("request failed: {e}")),
    }
}

impl DurableObject for EventsObject {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let cmd: LogCmd = command(req).await?;
        let store = SqlEvents::open(self.state.storage().sql())?;
        let outbox = SqlDeliveries::open(self.state.storage().sql())?;
        let queues = matches!(&cmd, LogCmd::Record { webhooks, .. } if !webhooks.is_empty());
        let out = eventlog::apply(&store, &outbox, cmd, now());
        if queues {
            self.arm(&outbox).await?;
        }
        reply(&out)
    }

    async fn alarm(&self) -> Result<Response> {
        let outbox = SqlDeliveries::open(self.state.storage().sql())?;
        let config = match settings::config(&self.env).await {
            Ok(c) => c,
            Err(e) => {
                worker::console_error!("deliveries: config unavailable: {e}");
                self.state.storage().set_alarm(CONFIG_RETRY).await?;
                return Response::ok("later");
            }
        };
        let switches = settings::switches(&self.env, Some(&config)).map_err(rust)?;
        let list = targets(&config, &switches);
        let lookup = |name: &str| settings::webhook_binding(&self.env, name);
        let bindings = Bindings {
            targets: &list,
            secret: &lookup,
        };
        let jobs = outbound::claim(&outbox, &bindings, now(), BATCH).map_err(rust)?;
        for (d, request) in jobs {
            let outcome = send(&request).await;
            let d = outbound::finish(&outbox, d, &request, outcome, now()).map_err(rust)?;
            if d.state != outbound::State::Delivered {
                worker::console_warn!(
                    "deliveries: {} to {} is {:?} after {} attempt(s)",
                    d.event_id,
                    d.webhook,
                    d.state,
                    d.attempts
                );
            }
        }
        self.arm(&outbox).await?;
        Response::ok("done")
    }
}
