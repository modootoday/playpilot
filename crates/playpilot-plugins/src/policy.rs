//! `policy-check` plugins: extra refusals after the core guard allowed a command.

use crate::entry::{Entry, Interface};
use crate::host::{Host, Loaded};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../schemas/playpilot-plugin.wit",
        world: "policy-plugin",
    });
}

use bindings::playpilot::fleet::types::Verdict;

/// A verified policy plugin, ready to check commands.
pub struct PolicyPlugin<'h> {
    host: &'h Host,
    loaded: Loaded,
}

impl Host {
    /// Loads a `policy-check` plugin. Policy plugins never get grants.
    pub fn load_policy(&self, entry: &Entry) -> Result<PolicyPlugin<'_>, String> {
        let loaded = self.load(entry, Interface::PolicyCheck)?;
        Ok(PolicyPlugin { host: self, loaded })
    }
}

impl PolicyPlugin<'_> {
    pub fn id(&self) -> &str {
        &self.loaded.id
    }

    /// `Ok` only when the plugin answers `allow`. A refusal, a trap, running
    /// out of fuel or memory, or a failed instantiation all refuse.
    pub fn check(&self, tool: &str, command: &str) -> Result<(), String> {
        let id = &self.loaded.id;
        let fail = |e: wasmtime::Error| format!("policy plugin {id} failed, refusing: {e}");
        let mut store = self.host.store()?;
        let plugin = bindings::PolicyPlugin::instantiate(
            &mut store,
            &self.loaded.component,
            &self.host.linker,
        )
        .map_err(fail)?;
        match plugin
            .playpilot_fleet_policy_check()
            .call_check_command(&mut store, tool, command)
            .map_err(fail)?
        {
            Verdict::Allow => Ok(()),
            Verdict::Refuse(reason) => Err(format!("policy plugin {id} refused: {reason}")),
        }
    }
}

/// Runs every plugin in order; the first refusal wins. Plugins can only
/// refuse, so the caller must have allowed the command already.
pub fn check_all(plugins: &[PolicyPlugin<'_>], tool: &str, command: &str) -> Result<(), String> {
    plugins.iter().try_for_each(|p| p.check(tool, command))
}
