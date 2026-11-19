//! `/v1/*` handlers (schemas/worker.v1.openapi.yaml).

use serde::Deserialize;
use serde_json::json;
use worker::{Env, Request, Response, Result};

use super::{call, http, settings};
use crate::domain::bus::{BusCmd, BusReply};
use crate::domain::config::not_modified;
use crate::domain::event;
use crate::domain::identity::Identity;
use crate::domain::ids::ItemRef;
use crate::domain::lease::{LeaseCmd, LeaseError, LeaseReply, Limits, clamp_ttl};
use crate::domain::node::{Heartbeat, NodeCmd, NodeReply};
use crate::domain::state;
use crate::domain::time::{duration, rfc3339};

pub async fn config(req: &Request, env: &Env) -> Result<Response> {
    let (compiled, ops) = match (settings::config(env).await, settings::worker_ops(env)) {
        (Ok(c), Ok(o)) => (c, o),
        (Err(e), _) | (_, Err(e)) => return http::internal(e),
    };
    let etag = compiled.etag(&ops);
    let asked = req.headers().get("If-None-Match").ok().flatten();
    let mut resp = if not_modified(asked.as_deref(), &etag) {
        http::empty(304)?
    } else {
        http::json(200, &compiled.serve(&ops))?
    };
    resp.headers_mut().set("ETag", &etag)?;
    Ok(resp)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseRequest {
    lane: String,
    item: ItemRef,
    holder: String,
    ttl: Option<String>,
}

fn lease_error(e: &LeaseError) -> Result<Response> {
    let (status, title) = e.status();
    http::problem(status, title, &e.detail())
}

pub async fn create_lease(mut req: Request, env: &Env, caller: &Identity) -> Result<Response> {
    let body = req.text().await?;
    let parsed: LeaseRequest = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => return http::problem(400, "Bad request", &e.to_string()),
    };
    if parsed.holder != caller.claim {
        return http::problem(403, "Forbidden", "holder must be the caller's own claim id");
    }
    let ttl = match parsed.ttl.as_deref().map(duration).transpose() {
        Ok(t) => clamp_ttl(t),
        Err(e) => Err(LeaseError::Invalid(e)),
    };
    let ttl = match ttl {
        Ok(t) => t,
        Err(e) => return lease_error(&e),
    };
    let compiled = settings::config(env).await.ok();
    let switches = match settings::switches(env, compiled.as_ref()) {
        Ok(s) => s,
        Err(e) => return http::internal(e),
    };
    let limits = Limits {
        lane: switches.effective("Lane", &parsed.lane),
        device: switches.effective("Device", &caller.claim),
    };
    let cmd = LeaseCmd::Acquire {
        lane: parsed.lane,
        item: parsed.item,
        holder: caller.claim.clone(),
        ttl,
        limits,
    };
    match call::leases(env, &cmd).await {
        Ok(LeaseReply::Lease(l)) => http::json(201, &l.to_api()),
        Ok(LeaseReply::Error(e)) => lease_error(&e),
        Ok(other) => http::internal(format!("unexpected reply {other:?}")),
        Err(e) => http::internal(e),
    }
}

pub async fn release_lease(env: &Env, caller: &Identity, id: &str) -> Result<Response> {
    let cmd = LeaseCmd::Release {
        id: id.to_string(),
        holder: caller.claim.clone(),
    };
    match call::leases(env, &cmd).await {
        Ok(LeaseReply::Lease(_)) => http::empty(204),
        Ok(LeaseReply::Error(e)) => lease_error(&e),
        Ok(other) => http::internal(format!("unexpected reply {other:?}")),
        Err(e) => http::internal(e),
    }
}

pub async fn heartbeat(
    mut req: Request,
    env: &Env,
    caller: &Identity,
    node: &str,
) -> Result<Response> {
    if node != caller.claim {
        return http::problem(403, "Forbidden", "a node reports only its own heartbeat");
    }
    let hb = match Heartbeat::parse(&req.text().await?) {
        Ok(h) => h,
        Err(e) => return http::problem(400, "Bad request", &e),
    };
    let leases = hb.leases.clone();
    let cmd = NodeCmd::Heartbeat {
        id: caller.claim.clone(),
        heartbeat: hb,
    };
    match call::nodes(env, &cmd).await {
        Ok(NodeReply::Ok) => {}
        Ok(NodeReply::Error(e)) => return http::problem(400, "Bad request", &e),
        Ok(other) => return http::internal(format!("unexpected reply {other:?}")),
        Err(e) => return http::internal(e),
    }
    if !leases.is_empty() {
        let renew = LeaseCmd::Renew {
            ids: leases,
            holder: caller.claim.clone(),
        };
        if let Err(e) = call::leases(env, &renew).await {
            return http::internal(e);
        }
    }
    let compiled = settings::config(env).await.ok();
    let switches = match settings::switches(env, compiled.as_ref()) {
        Ok(s) => s,
        Err(e) => return http::internal(e),
    };
    let mut ack = json!({
        "receivedAt": rfc3339(settings::now()),
        "ops": switches.effective("Device", &caller.claim),
    });
    if let (Some(c), Ok(o)) = (&compiled, settings::worker_ops(env)) {
        ack["configEtag"] = json!(c.etag(&o));
    }
    http::json(200, &ack)
}

pub async fn events(mut req: Request, env: &Env) -> Result<Response> {
    let content_type = req.headers().get("Content-Type")?.unwrap_or_default();
    let accepted = match event::parse(&content_type, &req.text().await?) {
        Ok(a) => a,
        Err(e) => return http::problem(400, "Bad request", &e),
    };
    let cmd = BusCmd::Publish {
        events: accepted.into_iter().map(|a| (a.key, a.json)).collect(),
    };
    match call::bus(env, &cmd).await {
        Ok(BusReply::Published(_)) => http::empty(202),
        Ok(other) => http::internal(format!("unexpected reply {other:?}")),
        Err(e) => http::internal(e),
    }
}

pub async fn state(env: &Env) -> Result<Response> {
    let nodes = match call::nodes(env, &NodeCmd::List).await {
        Ok(NodeReply::Nodes(n)) => n,
        Ok(other) => return http::internal(format!("unexpected reply {other:?}")),
        Err(e) => return http::internal(e),
    };
    let leased = match call::leases(env, &LeaseCmd::PerLane).await {
        Ok(LeaseReply::PerLane(m)) => m,
        Ok(other) => return http::internal(format!("unexpected reply {other:?}")),
        Err(e) => return http::internal(e),
    };
    let compiled = settings::config(env).await.ok();
    let switches = match settings::switches(env, compiled.as_ref()) {
        Ok(s) => s,
        Err(e) => return http::internal(e),
    };
    let lanes = compiled.as_ref().map(|c| c.lanes()).unwrap_or_default();
    http::json(
        200,
        &state::build(&state::Inputs {
            nodes: &nodes,
            leased: &leased,
            config_lanes: &lanes,
            switches: &switches,
            now: settings::now(),
            offline_after: settings::offline_after(env),
        }),
    )
}
