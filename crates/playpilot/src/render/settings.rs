//! `playpilot render settings`: writes a project's guard hook into its
//! `.claude/settings.json` from the fleet configuration.
//!
//! Inputs: the `Project` resource (protected branches) and `playpilot.lock`
//! (the pinned rev). Other settings keys and hooks are kept in their order.

use serde_json::{Map, Value, json};

use crate::validate::{self, Resource};

const REPO_URL: &str = "https://github.com/modootoday/playpilot";
const MATCHER: &str = "Bash|PowerShell";
const TIMEOUT_SECS: u64 = 240;

/// What the guard hook needs from the fleet configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardSpec {
    pub rev: String,
    pub protected: Vec<String>,
}

/// Reads `rev = "<sha>"` from `playpilot.lock`.
pub fn lock_rev(text: &str) -> Result<String, String> {
    let t: toml::Table = text
        .parse()
        .map_err(|e| format!("playpilot.lock: not TOML: {e}"))?;
    let rev = t
        .get("rev")
        .and_then(toml::Value::as_str)
        .ok_or("playpilot.lock: `rev` is required")?;
    if rev.len() != 40 || !rev.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "playpilot.lock: `rev` must be a full commit sha, got \"{rev}\""
        ));
    }
    Ok(rev.to_string())
}

/// Finds the named project and builds its guard spec.
pub fn guard_spec(resources: &[Resource], project: &str, rev: String) -> Result<GuardSpec, String> {
    let p = resources
        .iter()
        .find(|r| r.kind == "Project" && r.name == project)
        .ok_or_else(|| format!("no Project \"{project}\" in the fleet configuration"))?;
    let protected = validate::strings(&p.spec, "protected");
    if protected.is_empty() {
        return Err(format!(
            "{}: spec.protected must name at least one branch",
            p.file
        ));
    }
    if let Some(b) = protected.iter().find(|b| !is_branch_name(b)) {
        return Err(format!("{}: \"{b}\" is not a plain branch name", p.file));
    }
    Ok(GuardSpec { rev, protected })
}

fn is_branch_name(b: &str) -> bool {
    !b.is_empty()
        && b.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'/' | b'-' | b'_' | b'.'))
}

/// The hook command: installs the pinned rev when the recorded one differs,
/// fails closed when no CLI is available, then runs the guard.
pub fn guard_command(spec: &GuardSpec) -> String {
    format!(
        concat!(
            "sh -c 'R={rev}; B=\"$(command -v playpilot || echo \"$HOME/.cargo/bin/playpilot\")\"; ",
            "D=\"$(dirname \"$(dirname \"$B\")\")\"; ",
            "if [ \"$(cat \"$D/share/playpilot.rev\" 2>/dev/null)\" != \"$R\" ]; then ",
            "for c in cargo /root/.cargo/bin/cargo \"$HOME/.cargo/bin/cargo\"; do ",
            "command -v \"$c\" >/dev/null 2>&1 && \"$c\" install --force --locked --quiet ",
            "--git {url} --rev \"$R\" --root \"$D\" playpilot >&2 ",
            "&& mkdir -p \"$D/share\" && echo \"$R\" > \"$D/share/playpilot.rev\" && break; done; fi; ",
            "[ -x \"$B\" ] || {{ echo \"playpilot CLI is missing and could not be installed\" >&2; exit 2; }}; ",
            "PLAYPILOT_PROTECTED_BRANCHES={branches} exec \"$B\" guard'"
        ),
        rev = spec.rev,
        url = REPO_URL,
        branches = spec.protected.join(","),
    )
}

fn is_guard_hook(h: &Value) -> bool {
    h.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains("playpilot") && c.contains("guard"))
}

/// Merges the guard hook into existing settings text (or none) and returns
/// the new text. An existing guard hook is replaced in place; the
/// `PLAYPILOT_PROTECTED_BRANCHES` env entry is dropped because the command
/// carries the branches.
pub fn merge_settings(existing: Option<&str>, spec: &GuardSpec) -> Result<String, String> {
    let mut root: Map<String, Value> = match existing {
        Some(text) if !text.trim().is_empty() => match serde_json::from_str(text) {
            Ok(Value::Object(m)) => m,
            Ok(_) => return Err("settings.json: the top level must be an object".into()),
            Err(e) => return Err(format!("settings.json: not JSON: {e}")),
        },
        _ => Map::new(),
    };
    let hook = json!({"type": "command", "command": guard_command(spec), "timeout": TIMEOUT_SECS});

    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("settings.json: `hooks` must be an object")?;
    let pre = hooks
        .entry("PreToolUse")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or("settings.json: `hooks.PreToolUse` must be an array")?;

    let mut placed = false;
    for entry in pre.iter_mut() {
        let Some(list) = entry.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        let mut here = false;
        for h in list.iter_mut() {
            if is_guard_hook(h) {
                if placed {
                    *h = Value::Null;
                } else {
                    *h = hook.clone();
                    placed = true;
                    here = true;
                }
            }
        }
        list.retain(|h| !h.is_null());
        // Fix the matcher only when the entry holds nothing but the guard.
        if here
            && list.len() == 1
            && let Some(o) = entry.as_object_mut()
        {
            o.insert("matcher".into(), MATCHER.into());
        }
    }
    pre.retain(|e| {
        e.get("hooks")
            .and_then(Value::as_array)
            .is_none_or(|l| !l.is_empty())
    });
    if !placed {
        pre.push(json!({"matcher": MATCHER, "hooks": [hook]}));
    }

    if let Some(env) = root.get_mut("env").and_then(Value::as_object_mut) {
        env.remove("PLAYPILOT_PROTECTED_BRANCHES");
        if env.is_empty() {
            root.remove("env");
        }
    }
    let mut text = serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|e| format!("settings.json: {e}"))?;
    text.push('\n');
    Ok(text)
}
