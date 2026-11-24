//! Builds the test plugins at test time: a core module from WAT, wrapped
//! into a component for one world of `schemas/playpilot-plugin.wit`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use playpilot_plugins::{Entry, Interface, cache_path, sha256_hex};
use wit_component::{ComponentEncoder, StringEncoding, embed_component_metadata};
use wit_parser::Resolve;

const WIT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../schemas/playpilot-plugin.wit"
);

/// The component bytes for `wat` exporting `world`.
pub fn component(world: &str, wat: &str) -> Vec<u8> {
    let mut resolve = Resolve::default();
    let pkg = resolve.push_file(WIT).expect("WIT parses");
    let world = resolve.select_world(&[pkg], Some(world)).expect("world");
    let mut module = wat::parse_str(wat).expect("WAT parses");
    embed_component_metadata(&mut module, &resolve, world, StringEncoding::UTF8, false)
        .expect("metadata embeds");
    ComponentEncoder::default()
        .module(&module)
        .expect("module")
        .validate(true)
        .encode()
        .expect("component encodes")
}

/// A fresh, empty cache directory for one test.
pub fn cache_dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "playpilot-plugins-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cache dir");
    dir
}

/// Writes `bytes` to `<cache>/<sha256>.wasm` and returns an entry pinned to it.
pub fn cached(cache: &Path, id: &str, interface: Interface, bytes: &[u8]) -> Entry {
    let sha256 = sha256_hex(bytes);
    let path = cache_path(cache, &sha256);
    std::fs::write(&path, bytes).expect("write plugin");
    Entry {
        id: id.into(),
        interface,
        sha256,
        path,
        network: Vec::new(),
        secrets: Vec::new(),
    }
}

pub fn policy(cache: &Path, id: &str, wat: &str) -> Entry {
    cached(
        cache,
        id,
        Interface::PolicyCheck,
        &component("policy-plugin", wat),
    )
}

pub fn sink(cache: &Path, id: &str, wat: &str) -> Entry {
    cached(
        cache,
        id,
        Interface::EventSink,
        &component("sink-plugin", wat),
    )
}

pub const POLICY_DENY_RM: &str = include_str!("wat/policy-deny-rm.wat");
pub const POLICY_TRAP: &str = include_str!("wat/policy-trap.wat");
pub const POLICY_SPIN: &str = include_str!("wat/policy-spin.wat");
pub const POLICY_BIG_MEMORY: &str = include_str!("wat/policy-big-memory.wat");
pub const SINK_ECHO: &str = include_str!("wat/sink-echo.wat");
pub const SINK_TRAP: &str = include_str!("wat/sink-trap.wat");
