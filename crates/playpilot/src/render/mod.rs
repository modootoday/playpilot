//! `playpilot render`: projections of the fleet configuration.
//!
//! `settings` writes a project's guard hook, `prompt` prints a session's
//! scheduled-task prompt, `node` writes a device's service files.

pub mod cli;
pub mod node;
pub mod prompt;
pub mod settings;

use crate::validate::Resource;

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
