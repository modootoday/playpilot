//! `playpilot render plugins`: the plugin index (`index.json`) the plugins
//! host serves, built from the fleet's `Plugin` resources.
//!
//! Every entry is pinned by an https URL and a SHA-256 digest and lists only
//! what the plugin is granted: network hosts and secret names, never values.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use toml::Value as Toml;

use crate::validate::{self, Resource};

/// Index format version; bumped only on breaking changes.
pub const INDEX_VERSION: u64 = 1;

/// The WIT interfaces of `schemas/playpilot-plugin.wit` a plugin may export.
pub const INTERFACES: [&str; 4] = ["policy-check", "event-sink", "lane-source", "runtime"];

/// How a plugin runs.
pub const KINDS: [&str; 2] = ["wasm-component", "process"];

/// The WIT package every interface belongs to, and the major it supports.
const WIT_PACKAGE: &str = "playpilot:fleet/";
const WIT_MAJOR: &str = "1";

/// One plugin as the index lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub kind: String,
    pub interface: String,
    pub wit: String,
    pub url: String,
    pub sha256: String,
    pub network: Vec<String>,
    pub secrets: Vec<String>,
}

/// Reads every `Plugin` resource into an entry, sorted by id. Any problem
/// refuses the whole index, so a bad pin is never published.
pub fn entries(resources: &[Resource]) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for r in resources.iter().filter(|r| r.kind == "Plugin") {
        if !seen.insert(r.name.as_str()) {
            problems.push(format!("{}: duplicate Plugin \"{}\"", r.file, r.name));
            continue;
        }
        match entry(r) {
            Ok(e) => out.push(e),
            Err(e) => problems.push(format!("{}: {e}", r.file)),
        }
    }
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn entry(r: &Resource) -> Result<Entry, String> {
    let mut literal = Vec::new();
    validate::find_literal_secrets(&r.file, &r.spec, "spec", &mut literal);
    if let Some(first) = literal.first() {
        return Err(first
            .strip_prefix(&format!("{}: ", r.file))
            .unwrap_or(first)
            .to_string());
    }
    let kind = validate::string(&r.spec, "kind").unwrap_or_else(|| KINDS[0].into());
    if !KINDS.contains(&kind.as_str()) {
        return Err(format!("spec.kind \"{kind}\" is not one of {KINDS:?}"));
    }
    let wit = validate::string(&r.spec, "interface").ok_or("spec.interface is required")?;
    let interface = interface_name(&wit)?;
    let source = r
        .spec
        .get("source")
        .and_then(Toml::as_table)
        .ok_or("spec.source { url, sha256 } is required")?;
    let url = validate::string(source, "url").ok_or("spec.source.url is required")?;
    if !is_https_url(&url) {
        return Err(format!("spec.source.url \"{url}\" must be an https URL"));
    }
    let sha256 = validate::string(source, "sha256").ok_or("spec.source.sha256 is required")?;
    if !is_sha256(&sha256) {
        return Err("spec.source.sha256 must be 64 lower-case hex digits".into());
    }
    let (network, secrets) = grants(r.spec.get("grants"))?;
    Ok(Entry {
        id: r.name.clone(),
        kind,
        interface: interface.into(),
        wit,
        url,
        sha256,
        network,
        secrets,
    })
}

/// `playpilot:fleet/<interface>@1.<minor>.<patch>` to `<interface>`.
fn interface_name(wit: &str) -> Result<&str, String> {
    let unknown = || {
        format!(
            "spec.interface \"{wit}\" is not one of {WIT_PACKAGE}{{{}}}@{WIT_MAJOR}.x.y",
            INTERFACES.join(",")
        )
    };
    let rest = wit.strip_prefix(WIT_PACKAGE).ok_or_else(unknown)?;
    let (name, version) = rest.split_once('@').ok_or_else(unknown)?;
    let parts: Vec<&str> = version.split('.').collect();
    let numeric = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()));
    if !INTERFACES.contains(&name) || !numeric || parts[0] != WIT_MAJOR {
        return Err(unknown());
    }
    Ok(name)
}

fn is_https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !host.is_empty() && !host.contains('@') && url.bytes().all(|c| c.is_ascii_graphic())
}

pub fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        })
}

fn is_secret_name(s: &str) -> bool {
    s.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && s.bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

/// `grants = { network = [host...], secrets = [NAME | { secretRef = NAME }...] }`.
fn grants(v: Option<&Toml>) -> Result<(Vec<String>, Vec<String>), String> {
    let Some(v) = v else {
        return Ok((Vec::new(), Vec::new()));
    };
    let t = v.as_table().ok_or("spec.grants must be a table")?;
    if let Some(k) = t
        .keys()
        .find(|k| !matches!(k.as_str(), "network" | "secrets"))
    {
        return Err(format!("spec.grants.{k} is not a known grant"));
    }
    let network = list(t.get("network"), "network", |v| {
        v.as_str()
            .filter(|h| is_host(h))
            .map(str::to_string)
            .ok_or("must be a lower-case host name".into())
    })?;
    let secrets = list(t.get("secrets"), "secrets", secret_name)?;
    Ok((network, secrets))
}

/// A secret grant: a bare name or `{ secretRef = "NAME" }`; anything else
/// could carry a value and is refused.
fn secret_name(v: &Toml) -> Result<String, String> {
    let name = match v {
        Toml::String(s) => s.as_str(),
        Toml::Table(t) if t.len() == 1 => t
            .get("secretRef")
            .and_then(Toml::as_str)
            .ok_or("must be a secret name or { secretRef = \"NAME\" }")?,
        _ => return Err("must be a secret name or { secretRef = \"NAME\" }".into()),
    };
    if is_secret_name(name) {
        Ok(name.to_string())
    } else {
        Err("must be a secret name (A-Z, 0-9, _), never a value".into())
    }
}

fn list(
    v: Option<&Toml>,
    key: &str,
    item: impl Fn(&Toml) -> Result<String, String>,
) -> Result<Vec<String>, String> {
    let Some(v) = v else {
        return Ok(Vec::new());
    };
    let a = v
        .as_array()
        .ok_or_else(|| format!("spec.grants.{key} must be a list"))?;
    let mut out = BTreeSet::new();
    for (i, x) in a.iter().enumerate() {
        let s = item(x).map_err(|e| format!("spec.grants.{key}[{i}] {e}"))?;
        if !out.insert(s) {
            return Err(format!("spec.grants.{key}[{i}] is listed twice"));
        }
    }
    Ok(out.into_iter().collect())
}

/// The index as JSON: entries in id order, grants sorted.
pub fn index(entries: &[Entry]) -> Value {
    let plugins: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "kind": e.kind,
                "interface": e.interface,
                "wit": e.wit,
                "source": { "url": e.url, "sha256": e.sha256 },
                "grants": { "network": e.network, "secrets": e.secrets },
            })
        })
        .collect();
    json!({ "version": INDEX_VERSION, "plugins": plugins })
}

/// The `index.json` text: pretty JSON with a trailing newline.
pub fn index_text(resources: &[Resource]) -> Result<String, String> {
    let v = index(&entries(resources)?);
    let mut text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
#[path = "plugins_tests.rs"]
mod tests;
