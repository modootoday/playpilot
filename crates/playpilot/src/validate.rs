//! `playpilot validate <dir>`: checks a fleet configuration directory.
//!
//! Schema: every file is one `playpilot.io/v1` resource. Semantics: names are
//! unique per kind, references resolve, every lane a project uses can be
//! served by some device, and no literal secret values are present.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use toml::{Table, Value};

pub const API_VERSION: &str = "playpilot.io/v1";

pub(crate) const KINDS: [&str; 10] = [
    "Fleet",
    "Environment",
    "Project",
    "Lane",
    "Device",
    "Role",
    "PermissionProfile",
    "Session",
    "Webhook",
    "Plugin",
];

/// Profiles every fleet has without declaring them.
const BUILTIN_PROFILES: [&str; 2] = ["baseline", "node"];

#[derive(Debug, Clone)]
pub struct Resource {
    pub file: String,
    pub kind: String,
    pub name: String,
    pub spec: Table,
}

/// Parses one file's text into a resource, or explains why it is not one.
pub fn parse(file: &str, text: &str) -> Result<Resource, String> {
    let t: Table = text.parse().map_err(|e| format!("{file}: not TOML: {e}"))?;
    let api = t.get("apiVersion").and_then(Value::as_str);
    if api != Some(API_VERSION) {
        return Err(format!("{file}: apiVersion must be \"{API_VERSION}\""));
    }
    let kind = t.get("kind").and_then(Value::as_str).unwrap_or_default();
    if !KINDS.contains(&kind) {
        return Err(format!("{file}: unknown kind \"{kind}\""));
    }
    let name = t
        .get("metadata")
        .and_then(|m| m.get("name"))
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
        .ok_or_else(|| format!("{file}: metadata.name is required"))?;
    let spec = t
        .get("spec")
        .and_then(Value::as_table)
        .cloned()
        .ok_or_else(|| format!("{file}: spec is required"))?;
    Ok(Resource {
        file: file.into(),
        kind: kind.into(),
        name: name.into(),
        spec,
    })
}

pub fn strings(spec: &Table, key: &str) -> Vec<String> {
    spec.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn string(spec: &Table, key: &str) -> Option<String> {
    spec.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Semantic checks across all resources; empty means valid.
pub fn check(resources: &[Resource]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut names: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for r in resources {
        if !names
            .entry(r.kind.as_str())
            .or_default()
            .insert(r.name.as_str())
        {
            problems.push(format!("{}: duplicate {} \"{}\"", r.file, r.kind, r.name));
        }
        find_literal_secrets(&r.file, &r.spec, "spec", &mut problems);
    }
    let has = |kind: &str, name: &str| names.get(kind).is_some_and(|s| s.contains(name));
    let mut need = |r: &Resource, kind: &str, name: &str, field: &str| {
        let ok =
            has(kind, name) || (kind == "PermissionProfile" && BUILTIN_PROFILES.contains(&name));
        if !ok {
            problems.push(format!(
                "{}: {field} \"{name}\" is not a declared {kind}",
                r.file
            ));
        }
    };
    for r in resources {
        match r.kind.as_str() {
            "Device" => {
                for role in strings(&r.spec, "roles") {
                    need(r, "Role", &role, "roles");
                }
                for lane in strings(&r.spec, "primaryLanes") {
                    need(r, "Lane", &lane, "primaryLanes");
                }
            }
            "Project" => {
                for lane in strings(&r.spec, "lanes") {
                    need(r, "Lane", &lane, "lanes");
                }
            }
            "Role" => {
                if let Some(p) = string(&r.spec, "profile") {
                    need(r, "PermissionProfile", &p, "profile");
                }
            }
            "PermissionProfile" => {
                if let Some(p) = string(&r.spec, "extends") {
                    need(r, "PermissionProfile", &p, "extends");
                }
            }
            "Session" => {
                if let Some(p) = string(&r.spec, "project") {
                    need(r, "Project", &p, "project");
                }
                if let Some(p) = string(&r.spec, "role") {
                    need(r, "Role", &p, "role");
                }
            }
            _ => {}
        }
    }
    problems.extend(uncovered_lanes(resources));
    problems
}

/// A lane a project uses must have a device whose capabilities cover it.
fn uncovered_lanes(resources: &[Resource]) -> Vec<String> {
    let devices: Vec<BTreeSet<String>> = resources
        .iter()
        .filter(|r| r.kind == "Device")
        .map(|r| strings(&r.spec, "capabilities").into_iter().collect())
        .collect();
    let used: BTreeSet<String> = resources
        .iter()
        .filter(|r| r.kind == "Project")
        .flat_map(|r| strings(&r.spec, "lanes"))
        .collect();
    resources
        .iter()
        .filter(|r| r.kind == "Lane" && used.contains(&r.name))
        .filter(|lane| {
            let req: BTreeSet<String> = strings(&lane.spec, "requires").into_iter().collect();
            !devices.iter().any(|caps| req.is_subset(caps))
        })
        .map(|lane| format!("{}: no device can serve lane \"{}\"", lane.file, lane.name))
        .collect()
}

/// Secret-like keys must hold a `{ secretRef = "NAME" }`, never a value.
fn find_literal_secrets(file: &str, t: &Table, path: &str, out: &mut Vec<String>) {
    for (k, v) in t {
        let here = format!("{path}.{k}");
        let lower = k.to_ascii_lowercase();
        let secretish = [
            "token",
            "password",
            "secret",
            "apikey",
            "api_key",
            "privatekey",
        ]
        .iter()
        .any(|w| lower.contains(w))
            && lower != "secretref"
            && lower != "secrets";
        match v {
            Value::String(_) if secretish => out.push(format!(
                "{file}: {here} holds a literal value; use {{ secretRef = \"NAME\" }}"
            )),
            Value::Table(inner) => find_literal_secrets(file, inner, &here, out),
            _ => {}
        }
    }
}

/// Reads every `*.toml` under `dir` (recursively), skipping lock files.
pub fn read_dir(dir: &Path) -> Result<Vec<Resource>, Vec<String>> {
    let mut files = Vec::new();
    collect(dir, &mut files).map_err(|e| vec![format!("{}: {e}", dir.display())])?;
    files.sort();
    let mut resources = Vec::new();
    let mut errors = Vec::new();
    for f in files {
        let shown = f.display().to_string();
        match std::fs::read_to_string(&f) {
            Ok(text) => match parse(&shown, &text) {
                Ok(r) => resources.push(r),
                Err(e) => errors.push(e),
            },
            Err(e) => errors.push(format!("{shown}: {e}")),
        }
    }
    if errors.is_empty() {
        Ok(resources)
    } else {
        Err(errors)
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "toml") {
            out.push(path);
        }
    }
    Ok(())
}
