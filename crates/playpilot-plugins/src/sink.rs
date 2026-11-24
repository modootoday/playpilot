//! `event-sink` plugins: observe events; a failure never fails the caller.

use crate::entry::{Entry, Interface};
use crate::host::{Host, Loaded};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../schemas/playpilot-plugin.wit",
        world: "sink-plugin",
    });
}

pub use bindings::playpilot::fleet::types::Event;

/// A verified sink plugin.
pub struct SinkPlugin<'h> {
    host: &'h Host,
    loaded: Loaded,
}

impl Host {
    /// Loads an `event-sink` plugin. Network grants are refused until the
    /// host can enforce them.
    pub fn load_sink(&self, entry: &Entry) -> Result<SinkPlugin<'_>, String> {
        let loaded = self.load(entry, Interface::EventSink)?;
        Ok(SinkPlugin { host: self, loaded })
    }
}

impl SinkPlugin<'_> {
    pub fn id(&self) -> &str {
        &self.loaded.id
    }

    /// Delivers one event. A plugin error, trap or limit is returned as `Err`.
    pub fn deliver(&self, event: &Event) -> Result<(), String> {
        let id = &self.loaded.id;
        let fail = |e: wasmtime::Error| format!("event sink {id} failed: {e}");
        let mut store = self.host.store()?;
        let plugin = bindings::SinkPlugin::instantiate(
            &mut store,
            &self.loaded.component,
            &self.host.linker,
        )
        .map_err(fail)?;
        plugin
            .playpilot_fleet_event_sink()
            .call_deliver(&mut store, event)
            .map_err(fail)?
            .map_err(|e| format!("event sink {id} reported: {e}"))
    }
}

/// Delivers to every sink. Failures are logged on stderr and returned for
/// the caller to record; they never stop delivery to the other sinks.
pub fn deliver_all(sinks: &[SinkPlugin<'_>], event: &Event) -> Vec<String> {
    let mut failures = Vec::new();
    for sink in sinks {
        if let Err(e) = sink.deliver(event) {
            eprintln!("playpilot: {e}; continuing");
            failures.push(e);
        }
    }
    failures
}
