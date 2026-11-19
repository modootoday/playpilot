//! The project-side files `render` produces for one project checkout.
//!
//! `sync` walks [`outputs`]; a new project-side projection is added there.

use std::path::Path;

use super::output::Output;
use super::settings as guard;

/// Every file rendered into the checkout at `repo` for `project`.
pub fn outputs(config: &Path, project: &str, repo: &Path) -> Result<Vec<Output>, String> {
    Ok(vec![settings(config, project, repo)?])
}

/// `<repo>/.claude/settings.json` with the guard hook merged in.
pub fn settings(config: &Path, project: &str, repo: &Path) -> Result<Output, String> {
    let path = repo.join(".claude").join("settings.json");
    let lock_path = config.join("playpilot.lock");
    let lock =
        std::fs::read_to_string(&lock_path).map_err(|e| format!("{}: {e}", lock_path.display()))?;
    let rev = guard::lock_rev(&lock)?;
    let spec = guard::guard_spec(&super::fleet(config)?, project, rev)?;
    let existing = super::output::read(&path)?;
    let text = guard::merge_settings(existing.as_deref(), &spec)?;
    Ok(Output { path, text })
}
