//! `POST /hooks/github`: authenticated only by `X-Hub-Signature-256` under
//! `PLAYPILOT_GITHUB_WEBHOOK_SECRET` (never by Access or a bearer token).
//! Checks live in `crate::domain::hook`; this file reads the request and
//! stores the event in `EventsObject`, naming the outbound webhooks that
//! want it.

use serde_json::json;
use worker::{Env, Request, Response, Result};

use super::{call, http, settings};
use crate::domain::eventlog::{LogCmd, LogReply};
use crate::domain::hook::{self, Delivery, Refusal};
use crate::domain::outbound::target::{matching, targets};

fn refused(r: &Refusal) -> Result<Response> {
    http::problem(r.status, r.title, r.detail)
}

/// Outbound `Webhook` targets that want `ty`. A config that cannot be read
/// queues nothing; the event is still stored.
async fn outbound_webhooks(env: &Env, ty: &str) -> Vec<String> {
    let found = match settings::config(env).await {
        Ok(c) => settings::switches(env, Some(&c)).map(|s| targets(&c, &s)),
        Err(e) => Err(e),
    };
    match found {
        Ok(list) => matching(&list, ty),
        Err(e) => {
            worker::console_warn!("hooks: no outbound webhooks queued: {e}");
            Vec::new()
        }
    }
}

pub async fn github(mut req: Request, env: &Env) -> Result<Response> {
    let secret = settings::github_webhook_secret(env);
    let declared = req
        .headers()
        .get("Content-Length")?
        .and_then(|v| v.trim().parse::<u64>().ok());
    if let Err(r) = hook::precheck(secret.is_some(), declared) {
        if r.status == 503 {
            worker::console_error!("hooks: {} is not set", settings::GITHUB_WEBHOOK_SECRET);
        }
        return refused(&r);
    }
    let header = |name: &str| req.headers().get(name).ok().flatten();
    let (event, id, signature, content_type) = (
        header("X-GitHub-Event"),
        header("X-GitHub-Delivery"),
        header("X-Hub-Signature-256"),
        header("Content-Type"),
    );
    let body = req.bytes().await?;
    let delivery = Delivery {
        event: event.as_deref(),
        id: id.as_deref(),
        signature: signature.as_deref(),
        content_type: content_type.as_deref(),
    };
    let received = match hook::receive(secret.as_deref(), &delivery, &body, settings::now()) {
        Ok(r) => r,
        Err(r) => {
            worker::console_warn!("hooks: refused with {}", r.status);
            return refused(&r);
        }
    };
    let id = received.id.clone();
    let webhooks = outbound_webhooks(env, &received.ty).await;
    let cmd = LogCmd::Record {
        id: received.id,
        ty: received.ty,
        json: received.json,
        webhooks,
    };
    match call::events(env, &cmd).await {
        Ok(LogReply::Recorded { .. }) => http::json(202, &json!({"id": id, "status": "accepted"})),
        Ok(LogReply::Duplicate) => http::json(200, &json!({"id": id, "status": "duplicate"})),
        Ok(other) => http::internal(format!("unexpected reply {other:?}")),
        Err(e) => http::internal(e),
    }
}
