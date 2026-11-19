//! Configuration bindings. Every value is set by the owner at deploy time;
//! a missing or invalid identity map means no caller is recognised.

use worker::Env;

use crate::domain::access::policy::Policy;
use crate::domain::config::{Compiled, Source, config_text};
use crate::domain::identity::IdentityMap;
use crate::domain::node::DEFAULT_OFFLINE_AFTER;
use crate::domain::ops::{OpsTable, Switches};
use crate::domain::token::{TokenMap, directory};
use crate::domain::wake::WakeMap;

/// JSON map from Access client id to actor and claim id.
pub const IDENTITIES: &str = "PLAYPILOT_IDENTITIES";
/// The compiled configuration JSON (served by `/v1/config`) when the
/// `CONFIG` KV namespace is not bound. Text bindings stop near 5 kB.
pub const CONFIG: &str = "PLAYPILOT_CONFIG";
/// KV namespace holding the compiled configuration under `CONFIG_KEY`.
pub const CONFIG_KV: &str = "CONFIG";
pub const CONFIG_KEY: &str = "config.json";
/// Access team domain, a bare host name; the JWT issuer and key host.
pub const ACCESS_TEAM_DOMAIN: &str = "PLAYPILOT_ACCESS_TEAM_DOMAIN";
/// Access application audience (AUD tag) the JWT must carry.
pub const ACCESS_AUD: &str = "PLAYPILOT_ACCESS_AUD";
/// Worker-side narrow-only ops, keyed `Kind/name`.
pub const OPS: &str = "PLAYPILOT_OPS";
/// JSON map from the SHA-256 of a `/mcp` bearer token to its actors.
pub const MCP_TOKENS: &str = "PLAYPILOT_MCP_TOKENS";
/// JSON map from actor to the routine fired when it gets mail.
pub const WAKE: &str = "PLAYPILOT_WAKE";
/// Minutes without a heartbeat before a node is offline.
pub const OFFLINE_MINUTES: &str = "PLAYPILOT_OFFLINE_MINUTES";

/// A plain var or a secret with that name.
fn text(env: &Env, name: &str) -> Option<String> {
    env.var(name)
        .map(|v| v.to_string())
        .or_else(|_| env.secret(name).map(|s| s.to_string()))
        .ok()
        .filter(|s| !s.trim().is_empty())
}

pub fn identities(env: &Env) -> Result<IdentityMap, String> {
    IdentityMap::parse(&text(env, IDENTITIES).ok_or(format!("{IDENTITIES} is not set"))?)
}

pub fn tokens(env: &Env) -> Result<TokenMap, String> {
    TokenMap::parse(&text(env, MCP_TOKENS).ok_or(format!("{MCP_TOKENS} is not set"))?)
}

/// Routines to fire on new mail; unset means no actor is woken.
pub fn wake(env: &Env) -> Result<WakeMap, String> {
    text(env, WAKE).map_or(Ok(WakeMap::default()), |t| WakeMap::parse(&t))
}

/// Broadcast directory: actors from the Access identities and the tokens.
pub fn mail_directory(env: &Env) -> Vec<String> {
    directory(identities(env).ok().as_ref(), tokens(env).ok().as_ref())
}

/// Unset or invalid values mean no `/v1/*` caller is accepted.
pub fn access_policy(env: &Env) -> Result<Policy, String> {
    Policy::new(
        text(env, ACCESS_TEAM_DOMAIN).as_deref(),
        text(env, ACCESS_AUD).as_deref(),
    )
}

/// The compiled config: KV when the namespace is bound, else the var.
pub async fn config(env: &Env) -> Result<Compiled, String> {
    let source = match env.kv(CONFIG_KV) {
        Ok(store) => Source::Kv(
            store
                .get(CONFIG_KEY)
                .text()
                .await
                .map_err(|e| format!("{CONFIG_KV}: {e}"))?,
        ),
        Err(_) => Source::Var(text(env, CONFIG)),
    };
    Compiled::parse(&config_text(source, CONFIG_KV, CONFIG_KEY, CONFIG)?)
}

pub fn worker_ops(env: &Env) -> Result<OpsTable, String> {
    text(env, OPS).map_or(Ok(OpsTable::default()), |t| OpsTable::parse(&t))
}

/// File ops from the compiled config narrowed by Worker ops. A Worker ops
/// value that does not parse is an error, never a silent widening.
pub fn switches(env: &Env, config: Option<&Compiled>) -> Result<Switches, String> {
    let file = config.map(Compiled::file_ops).unwrap_or_default();
    Ok(Switches {
        file,
        worker: worker_ops(env)?,
    })
}

pub fn offline_after(env: &Env) -> u64 {
    text(env, OFFLINE_MINUTES)
        .and_then(|m| m.trim().parse::<u64>().ok())
        .filter(|m| *m > 0)
        .map_or(DEFAULT_OFFLINE_AFTER, |m| m * 60)
}

pub fn now() -> u64 {
    worker::Date::now().as_millis() / 1_000
}
