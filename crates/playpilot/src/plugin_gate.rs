//! Policy plugins for `guard`: consulted only after the core policy allowed
//! a shell command. They can refuse, never allow what the core refused.

use std::ffi::OsStr;

use crate::policy::Refusal;

/// The variable naming the policy plugins: `sha256=path` items separated by `;`.
pub const ENV: &str = "PLAYPILOT_POLICY_PLUGINS";

/// Checks `command` against the plugins in `value` (the variable's value).
/// Unset or blank means no plugins. Anything else that cannot be checked
/// refuses: a non-UTF-8 value, a malformed list, a bad pin, a failing
/// plugin, or a build without the plugin host.
pub fn check(value: Option<&OsStr>, tool: &str, command: &str) -> Result<(), Refusal> {
    let Some(value) = value else {
        return Ok(());
    };
    let Some(value) = value.to_str() else {
        return Err(refuse(&format!("{ENV} is not valid UTF-8")));
    };
    if value.trim().is_empty() {
        return Ok(());
    }
    run(value, tool, command)
}

#[cfg(feature = "plugins")]
fn run(value: &str, tool: &str, command: &str) -> Result<(), Refusal> {
    playpilot_plugins::check_env(value, tool, command).map_err(|e| refuse(&e))
}

#[cfg(not(feature = "plugins"))]
fn run(_value: &str, _tool: &str, _command: &str) -> Result<(), Refusal> {
    Err(refuse(&format!(
        "{ENV} is set but this build has no plugin host; rebuild with `--features plugins` or unset it"
    )))
}

fn refuse(msg: &str) -> Refusal {
    Refusal(format!(
        "playpilot guard: {msg}; refusing (policy plugins fail closed)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_or_blank_checks_nothing() {
        assert!(check(None, "Bash", "ls").is_ok());
        assert!(check(Some(OsStr::new("  ")), "Bash", "ls").is_ok());
    }

    #[test]
    fn a_set_value_that_cannot_be_checked_refuses() {
        assert!(check(Some(OsStr::new("not-a-list")), "Bash", "ls").is_err());
        let missing = format!("{}=does-not-exist.wasm", "a".repeat(64));
        let err = check(Some(OsStr::new(&missing)), "Bash", "ls").unwrap_err();
        assert!(err.0.contains("fail closed"), "{}", err.0);
    }

    #[cfg(not(feature = "plugins"))]
    #[test]
    fn without_the_host_a_set_value_refuses() {
        let value = format!("{}=p.wasm", "a".repeat(64));
        let err = check(Some(OsStr::new(&value)), "Bash", "ls").unwrap_err();
        assert!(err.0.contains("--features plugins"), "{}", err.0);
    }

    #[cfg(feature = "plugins")]
    #[test]
    fn with_the_host_a_wrong_pin_refuses() {
        let dir = std::env::temp_dir().join(format!("playpilot-gate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.wasm");
        std::fs::write(&path, b"\0asm").unwrap();
        let value = format!("{}={}", "0".repeat(64), path.display());
        let err = check(Some(OsStr::new(&value)), "Bash", "ls").unwrap_err();
        assert!(err.0.contains("SHA-256 mismatch"), "{}", err.0);
    }
}
