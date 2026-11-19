//! Rendered files and where they go: one pure decision per file, then the
//! write. Shared by `render`, `sync` and `node install`.

use std::path::{Path, PathBuf};

/// A rendered file and the path it belongs at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub path: PathBuf,
    pub text: String,
}

/// How a command treats a target that differs from the rendered text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Report only (`--check`).
    Check,
    /// The file is generated; replace whatever is there (`render`, `sync`).
    Write,
    /// The file may hold local edits; replace a different one only with `force`.
    Install { force: bool },
}

/// What happens to one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    UpToDate,
    Stale,
    Write,
    Refuse,
}

/// Decides from the current text (`None` when the file is missing).
pub fn decide(existing: Option<&str>, wanted: &str, mode: Mode) -> Action {
    if existing == Some(wanted) {
        return Action::UpToDate;
    }
    match mode {
        Mode::Check => Action::Stale,
        Mode::Write | Mode::Install { force: true } => Action::Write,
        Mode::Install { force: false } if existing.is_none() => Action::Write,
        Mode::Install { force: false } => Action::Refuse,
    }
}

/// Reads a target; a missing file is `None`, any other failure is an error so
/// an unreadable file is never taken for a missing one.
pub fn read(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Decides for `out` and writes it when the decision is [`Action::Write`].
pub fn apply(out: &Output, mode: Mode) -> Result<Action, String> {
    let action = decide(read(&out.path)?.as_deref(), &out.text, mode);
    if action == Action::Write {
        if let Some(dir) = out.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&out.path, &out.text).map_err(|e| format!("{}: {e}", out.path.display()))?;
    }
    Ok(action)
}
