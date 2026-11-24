//! Plugin host for playpilot: runs `wasm-component` plugins that export
//! `policy-check` or `event-sink` from `schemas/playpilot-plugin.wit`.
//!
//! Plugins get no imports at all (no WASI filesystem, clock or network), a
//! fixed fuel budget and a memory cap. A policy plugin that fails, traps or
//! runs out of fuel refuses (fail closed); a sink that fails is logged and
//! the caller continues (fail open).

mod entry;
mod host;
mod policy;
mod sink;

pub use entry::{Entry, Interface, cache_path, parse_policy_env, sha256_hex};
pub use host::{Host, Limits};
pub use policy::{PolicyPlugin, check_all};
pub use sink::{Event, SinkPlugin, deliver_all};

/// Checks one command against the policy plugins named in
/// `PLAYPILOT_POLICY_PLUGINS` (`sha256=path;...`). Any problem refuses.
pub fn check_env(value: &str, tool: &str, command: &str) -> Result<(), String> {
    let entries = parse_policy_env(value)?;
    let host = Host::new(Limits::default())?;
    let plugins = entries
        .iter()
        .map(|e| host.load_policy(e))
        .collect::<Result<Vec<_>, _>>()?;
    check_all(&plugins, tool, command)
}
