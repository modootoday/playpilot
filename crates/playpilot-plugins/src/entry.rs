//! Which plugin to load: a plugin index entry resolved to a cached file.

use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The interfaces this host runs. `lane-source` and `runtime` are not hosted yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interface {
    PolicyCheck,
    EventSink,
}

impl Interface {
    pub fn name(self) -> &'static str {
        match self {
            Self::PolicyCheck => "policy-check",
            Self::EventSink => "event-sink",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "policy-check" => Ok(Self::PolicyCheck),
            "event-sink" => Ok(Self::EventSink),
            other => Err(format!("interface \"{other}\" is not hosted")),
        }
    }
}

/// One plugin to load: its pin, its grants and where its bytes are cached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub interface: Interface,
    pub sha256: String,
    pub path: PathBuf,
    pub network: Vec<String>,
    pub secrets: Vec<String>,
}

impl Entry {
    /// Reads one `plugins[]` entry of `index.json` and resolves it to
    /// `<cache>/<sha256>.wasm`. Fetching the URL is not done here.
    pub fn from_index(v: &Value, cache: &Path) -> Result<Self, String> {
        let s = |ptr: &str| {
            v.pointer(ptr)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("plugin entry: {ptr} is missing"))
        };
        let id = s("/id")?.to_string();
        let kind = s("/kind")?;
        if kind != "wasm-component" {
            return Err(format!("plugin {id}: kind \"{kind}\" is not hosted"));
        }
        let interface =
            Interface::parse(s("/interface")?).map_err(|e| format!("plugin {id}: {e}"))?;
        let sha256 = s("/source/sha256")?.to_string();
        if !is_sha256(&sha256) {
            return Err(format!(
                "plugin {id}: sha256 must be 64 lower-case hex digits"
            ));
        }
        let names = |ptr: &str| -> Result<Vec<String>, String> {
            match v.pointer(ptr) {
                None => Ok(Vec::new()),
                Some(Value::Array(a)) => a
                    .iter()
                    .map(|x| {
                        x.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| format!("plugin {id}: {ptr} must list strings"))
                    })
                    .collect(),
                Some(_) => Err(format!("plugin {id}: {ptr} must be a list")),
            }
        };
        Ok(Self {
            path: cache_path(cache, &sha256),
            network: names("/grants/network")?,
            secrets: names("/grants/secrets")?,
            id,
            interface,
            sha256,
        })
    }
}

/// Where a pinned plugin is cached: `<cache>/<sha256>.wasm`.
pub fn cache_path(cache: &Path, sha256: &str) -> PathBuf {
    cache.join(format!("{sha256}.wasm"))
}

/// Lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// `PLAYPILOT_POLICY_PLUGINS`: `sha256=path` items separated by `;`. Every
/// item is a policy plugin with no grants. A malformed item refuses all.
pub fn parse_policy_env(value: &str) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    for (i, item) in value.split(';').map(str::trim).enumerate() {
        if item.is_empty() {
            continue;
        }
        let (sha256, path) = item
            .split_once('=')
            .ok_or_else(|| format!("PLAYPILOT_POLICY_PLUGINS item {i} is not sha256=path"))?;
        let sha256 = sha256.trim();
        let path = path.trim();
        if !is_sha256(sha256) || path.is_empty() {
            return Err(format!(
                "PLAYPILOT_POLICY_PLUGINS item {i} needs a 64-hex sha256 and a path"
            ));
        }
        out.push(Entry {
            id: format!("policy[{i}]"),
            interface: Interface::PolicyCheck,
            sha256: sha256.to_string(),
            path: PathBuf::from(path),
            network: Vec::new(),
            secrets: Vec::new(),
        });
    }
    if out.is_empty() {
        return Err("PLAYPILOT_POLICY_PLUGINS is set but names no plugin".into());
    }
    Ok(out)
}
