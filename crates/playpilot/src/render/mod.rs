//! `playpilot render`: projections of the fleet configuration.
//!
//! `settings` writes a project's guard hook, `roles` its agent files,
//! `prompt` prints a session's scheduled-task prompt, `node` writes a
//! device's service files.
//! `output` and `project` are shared with `sync` and `node install`.

pub mod cli;
pub mod node;
pub mod output;
pub mod project;
pub mod prompt;
pub mod roles;
pub mod settings;

use std::path::Path;

use crate::validate::{self, Resource};

/// Reads `<config>/fleet`, the resources of the fleet configuration repository.
pub fn fleet(config: &Path) -> Result<Vec<Resource>, String> {
    validate::read_dir(&config.join("fleet")).map_err(|e| e.join("\n"))
}

/// Finds the resource of `kind` named `name`.
fn find<'a>(resources: &'a [Resource], kind: &str, name: &str) -> Result<&'a Resource, String> {
    resources
        .iter()
        .find(|r| r.kind == kind && r.name == name)
        .ok_or_else(|| format!("no {kind} \"{name}\" in the fleet configuration"))
}

/// A value that can sit in a prompt, unit file or XML without quoting.
fn is_plain(v: &str) -> bool {
    !v.is_empty()
        && v.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b':'))
}
