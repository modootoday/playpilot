//! `playpilot migrate`: rewrites fleet resources between API versions.
//!
//! Layers: the registry, planning and per-file rewrite below are pure
//! (domain); `cli` reads and writes the files (presentation). A step is one
//! function from one version's table to the next; the planner chains steps
//! and sets `apiVersion` after each one.

pub mod cli;

use toml::{Table, Value};

use crate::validate::API_VERSION;

/// Rewrites one resource table from `Step::from` to `Step::to` in place.
/// The planner sets `apiVersion`; a step changes only the other fields.
pub type Rewrite = fn(&mut Table) -> Result<(), String>;

#[derive(Debug, Clone, Copy)]
pub struct Step {
    pub from: &'static str,
    pub to: &'static str,
    pub rewrite: Rewrite,
}

/// Known API versions (oldest first) and the steps between them.
pub struct Registry {
    pub versions: Vec<&'static str>,
    pub steps: Vec<Step>,
}

impl Registry {
    /// The versions and steps this build knows. A future `v1 -> v2` adds
    /// `"playpilot.io/v2"` to `versions` and one `Step` to `steps`.
    pub fn builtin() -> Self {
        Registry {
            versions: vec![API_VERSION],
            steps: Vec::new(),
        }
    }

    pub fn newest(&self) -> &'static str {
        self.versions.last().copied().unwrap_or(API_VERSION)
    }

    fn index(&self, version: &str) -> Option<usize> {
        self.versions.iter().position(|v| *v == version)
    }

    /// The steps from `from` to `to`, in order; empty when they are equal.
    pub fn plan(&self, from: &str, to: &str) -> Result<Vec<Step>, String> {
        let target = self
            .index(to)
            .ok_or_else(|| format!("unknown target apiVersion \"{to}\""))?;
        let start = self
            .index(from)
            .ok_or_else(|| format!("unknown apiVersion \"{from}\""))?;
        if start > target {
            return Err(format!(
                "apiVersion \"{from}\" is newer than the target \"{to}\"; migrate does not downgrade"
            ));
        }
        let mut steps = Vec::new();
        let mut at = from;
        while at != to {
            let step = self
                .steps
                .iter()
                .find(|s| s.from == at)
                .filter(|s| self.index(s.to).is_some_and(|i| i <= target))
                .ok_or_else(|| format!("no migration step from \"{at}\" toward \"{to}\""))?;
            if steps.len() >= self.versions.len() {
                return Err(format!("migration steps from \"{from}\" form a loop"));
            }
            steps.push(*step);
            at = step.to;
        }
        Ok(steps)
    }
}

/// What migrating one file would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Already at the target with nothing to rewrite.
    Unchanged,
    /// The new file text; `lossy` when the rewrite drops comments.
    Rewrite {
        from: String,
        text: String,
        lossy: bool,
    },
}

/// Migrates one file's text to `to`. Pure: no file system access.
pub fn migrate_text(
    registry: &Registry,
    file: &str,
    text: &str,
    to: &str,
) -> Result<Outcome, String> {
    let original: Table = text.parse().map_err(|e| format!("{file}: not TOML: {e}"))?;
    let from = original
        .get("apiVersion")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{file}: apiVersion is required"))?
        .to_string();
    let steps = registry
        .plan(&from, to)
        .map_err(|e| format!("{file}: {e}"))?;
    let mut table = original.clone();
    for step in &steps {
        (step.rewrite)(&mut table)
            .map_err(|e| format!("{file}: {} -> {}: {e}", step.from, step.to))?;
        table.insert("apiVersion".into(), Value::String(step.to.into()));
    }
    if table == original {
        return Ok(Outcome::Unchanged);
    }
    if let Some(kept) = bump_in_place(text, &original, &table, to) {
        return Ok(Outcome::Rewrite {
            from,
            text: kept,
            lossy: false,
        });
    }
    let new = toml::to_string(&table).map_err(|e| format!("{file}: cannot write TOML: {e}"))?;
    Ok(Outcome::Rewrite {
        from,
        lossy: has_comments(text),
        text: new,
    })
}

/// When only `apiVersion` changed, edits that one line and keeps the rest of
/// the text, comments included.
fn bump_in_place(text: &str, original: &Table, migrated: &Table, to: &str) -> Option<String> {
    let mut only_api = original.clone();
    only_api.insert("apiVersion".into(), Value::String(to.into()));
    if &only_api != migrated {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut done = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let is_api = trimmed
            .strip_prefix("apiVersion")
            .is_some_and(|rest| rest.trim_start().starts_with('='));
        if !done && is_api {
            let indent = &line[..line.len() - trimmed.len()];
            let value_start = trimmed.find('=')? + 1;
            let after = &trimmed[value_start..];
            let quote_open = after.find('"')?;
            let quote_close = after[quote_open + 1..].find('"')? + quote_open + 1;
            out.push_str(indent);
            out.push_str(&trimmed[..value_start]);
            out.push_str(&after[..quote_open]);
            out.push_str(&Value::String(to.into()).to_string());
            out.push_str(&after[quote_close + 1..]);
            done = true;
        } else {
            out.push_str(line);
        }
    }
    let reparsed: Table = out.parse().ok()?;
    (done && &reparsed == migrated).then_some(out)
}

/// Whether TOML text holds a comment: a `#` outside every string form.
pub fn has_comments(text: &str) -> bool {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'#' => return true,
            q @ (b'"' | b'\'') => {
                let triple = b.get(i..i + 3) == Some(&[q, q, q][..]);
                let width = if triple { 3 } else { 1 };
                i += width;
                while i < b.len() {
                    if q == b'"' && b[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if !triple && b[i] == b'\n' {
                        break;
                    }
                    if b[i] == q && (!triple || b.get(i..i + 3) == Some(&[q, q, q][..])) {
                        i += width;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    false
}
