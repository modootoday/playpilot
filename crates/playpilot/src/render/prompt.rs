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
    })
}

/// The prompt text, one instruction per line.
pub fn prompt_text(s: &SessionSpec) -> String {
    format!(
        concat!(
            "Scheduled run of fleet session {name} (project {project}, role {role}, work {work}).\n",
            "Follow the repository's AGENTS.md and its automation rules; run the unattended loop once ",
            "for this work: take one item, finish it through a pull request, and report the result.\n",
            "Read your fleet mailbox with `playpilot mail inbox --as {project}` before choosing work.\n",
            "Do not ask the user questions; if there is nothing to do, say so in one line and stop.\n"
        ),
        name = s.name,
        project = s.project,
        role = s.role,
        work = s.work,
    )
}
