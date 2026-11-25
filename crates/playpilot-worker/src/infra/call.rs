//! Router → Durable Object calls: one JSON command in, one JSON reply out.

use serde::Serialize;
use serde::de::DeserializeOwned;
use worker::wasm_bindgen::JsValue;
use worker::{Env, Method, Request, RequestInit};

use crate::domain::bus::{BusCmd, BusReply};
use crate::domain::eventlog::{LogCmd, LogReply};
use crate::domain::lease::{LeaseCmd, LeaseReply};
use crate::domain::node::{NodeCmd, NodeReply};

/// Each class has a single global instance, so one object sees every lease,
/// node or mailbox and can enforce one holder per item and capacity limits.
const INSTANCE: &str = "fleet";

async fn call<C: Serialize, R: DeserializeOwned>(
    env: &Env,
    binding: &str,
    cmd: &C,
) -> Result<R, String> {
    let body = serde_json::to_string(cmd).map_err(|e| e.to_string())?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_body(Some(JsValue::from_str(&body)));
    let req =
        Request::new_with_init("https://do.internal/cmd", &init).map_err(|e| e.to_string())?;
    let stub = env
        .durable_object(binding)
        .and_then(|ns| ns.get_by_name(INSTANCE))
        .map_err(|e| e.to_string())?;
    let mut resp = stub
        .fetch_with_request(req)
        .await
        .map_err(|e| e.to_string())?;
    let text = resp.text().await.map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("{binding}: {e}"))
}

pub async fn leases(env: &Env, cmd: &LeaseCmd) -> Result<LeaseReply, String> {
    call(env, "LEASES", cmd).await
}

pub async fn nodes(env: &Env, cmd: &NodeCmd) -> Result<NodeReply, String> {
    call(env, "NODES", cmd).await
}

pub async fn bus(env: &Env, cmd: &BusCmd) -> Result<BusReply, String> {
    call(env, "BUS", cmd).await
}

pub async fn events(env: &Env, cmd: &LogCmd) -> Result<LogReply, String> {
    call(env, "EVENTS", cmd).await
}
