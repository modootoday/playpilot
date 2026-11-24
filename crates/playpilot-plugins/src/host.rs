//! The sandbox: one engine, an empty linker, fuel and a memory cap.

use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

use crate::entry::{Entry, Interface, sha256_hex};

/// Per-call resource limits. Running out of either is a failure.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Fuel for one call, instantiation included (about one unit per instruction).
    pub fuel: u64,
    /// Largest linear memory a plugin may have, in bytes.
    pub memory_bytes: usize,
    /// Largest plugin file the host reads, in bytes.
    pub max_file_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            fuel: 50_000_000,
            memory_bytes: 16 << 20,
            max_file_bytes: 16 << 20,
        }
    }
}

/// Compiles and runs plugins. Nothing is linked in: a component that
/// imports anything (WASI, sockets, host calls) fails to instantiate.
pub struct Host {
    pub(crate) engine: Engine,
    pub(crate) linker: Linker<StoreLimits>,
    limits: Limits,
}

/// A verified, compiled plugin.
pub(crate) struct Loaded {
    pub id: String,
    pub component: Component,
}

impl Host {
    pub fn new(limits: Limits) -> Result<Self, String> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.wasm_component_model(true);
        let engine = Engine::new(&config).map_err(|e| format!("plugin host: {e}"))?;
        let linker = Linker::new(&engine);
        Ok(Self {
            engine,
            linker,
            limits,
        })
    }

    /// A fresh store for one call: fuel set, memory capped, growth past the cap traps.
    pub(crate) fn store(&self) -> Result<Store<StoreLimits>, String> {
        let limits = StoreLimitsBuilder::new()
            .memory_size(self.limits.memory_bytes)
            .instances(16)
            .tables(16)
            .memories(4)
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(&self.engine, limits);
        store.limiter(|l| l);
        store
            .set_fuel(self.limits.fuel)
            .map_err(|e| format!("plugin host: {e}"))?;
        Ok(store)
    }

    /// Checks the grants and the SHA-256 pin, then compiles the component.
    pub(crate) fn load(&self, entry: &Entry, want: Interface) -> Result<Loaded, String> {
        let id = &entry.id;
        if entry.interface != want {
            return Err(format!(
                "plugin {id}: exports {} where {} is needed",
                entry.interface.name(),
                want.name()
            ));
        }
        if !entry.network.is_empty() {
            return Err(format!(
                "plugin {id}: network grants are not supported by this host"
            ));
        }
        if !entry.secrets.is_empty() {
            return Err(format!(
                "plugin {id}: secret grants are not supported by this host"
            ));
        }
        let path = entry.path.display();
        let size = std::fs::metadata(&entry.path)
            .map_err(|e| format!("plugin {id}: cannot read {path}: {e}"))?
            .len();
        if size > self.limits.max_file_bytes {
            return Err(format!(
                "plugin {id}: {path} is larger than the host allows"
            ));
        }
        let bytes = std::fs::read(&entry.path)
            .map_err(|e| format!("plugin {id}: cannot read {path}: {e}"))?;
        let actual = sha256_hex(&bytes);
        if actual != entry.sha256 {
            return Err(format!(
                "plugin {id}: SHA-256 mismatch for {path} (pinned {}, found {actual})",
                entry.sha256
            ));
        }
        let component = Component::from_binary(&self.engine, &bytes)
            .map_err(|e| format!("plugin {id}: not a valid component: {e}"))?;
        Ok(Loaded {
            id: id.clone(),
            component,
        })
    }
}
