//! `playpilot render prompt`: the scheduled-task prompt for a `Session`.
//!
//! The prompt carries no policy. It names the session and points it at the
//! repository's own rules, so rules change in one place.

use crate::validate::Resource;

/// What the prompt needs from a `Session` resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSpec {
    pub name: String,
    pub project: String,
    pub role: String,
    pub work: String,
    /// Project-specific standing lines (`spec.notes`), kept verbatim.
    pub notes: Vec<String>,
}

/// Longest note line, so a config value cannot smuggle a long instruction block.
const MAX_NOTE: usize = 300;

fn notes(s: &crate::validate::Resource) -> Result<Vec<String>, String> {
    let Some(v) = s.spec.get("notes") else {
        return Ok(Vec::new());
    };
    let arr = v
        .as_array()
        .ok_or_else(|| format!("{}: spec.notes must be a list of strings", s.file))?;
    arr.iter()
        .map(|n| {
            let t = n
                .as_str()
                .ok_or_else(|| format!("{}: spec.notes must be a list of strings", s.file))?
                .trim();
            if t.is_empty() || t.len() > MAX_NOTE || t.chars().any(char::is_control) {
                return Err(format!(
                    "{}: each spec.notes entry must be one line of 1-{MAX_NOTE} characters",
                    s.file
                ));
            }
            Ok(t.to_string())
        })
        .collect()
}

/// Finds the named session and reads the fields the prompt names.
pub fn session_spec(resources: &[Resource], name: &str) -> Result<SessionSpec, String> {
    let s = super::find(resources, "Session", name)?;
    let field = |key: &str| -> Result<String, String> {
        let v = s
            .spec
            .get(key)
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("{}: spec.{key} is required", s.file))?;
        if !super::is_plain(v) {
            return Err(format!(
                "{}: spec.{key} \"{v}\" is not a plain name",
                s.file
            ));
        }
        Ok(v.to_string())
    };
    Ok(SessionSpec {
        name: s.name.clone(),
        project: field("project")?,
        role: field("role")?,
        work: field("work")?,
        notes: notes(s)?,
    })
}

/// The prompt text, one instruction per line.
pub fn prompt_text(s: &SessionSpec) -> String {
    let mut text = format!(
        concat!(
            "Scheduled run of fleet session {name} (project {project}, role {role}, work {work}).\n",
            "Follow the repository's AGENTS.md and its automation rules; run the unattended loop once ",
            "for this work: take one item, finish it through a pull request, and report the result.\n",
            "Read your fleet mailbox with `playpilot mail inbox --as {project}` before choosing work.\n",
            "If the fleet configuration repository is checked out next to this one, run ",
            "`playpilot sync --check --project {project} --config <that checkout> --repo .`; when it ",
            "reports stale files, run it without `--check` and open one pull request with the result.\n",
            "In commits, pull requests and issues call it only \"the fleet configuration\": never its ",
            "repository name, path or commit.\n",
        ),
        name = s.name,
        project = s.project,
        role = s.role,
        work = s.work,
    );
    for note in &s.notes {
        text.push_str(note);
        text.push('\n');
    }
    text.push_str(
        "Do not ask the user questions; if there is nothing to do, say so in one line and stop.\n",
    );
    text
}
