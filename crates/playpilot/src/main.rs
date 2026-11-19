//! `playpilot guard`: a Claude Code PreToolUse hook.
//!
//! Reads the hook event JSON on stdin. Exit 0 allows the tool call; exit 2
//! blocks it and the stderr text is shown to Claude.

mod history;
mod policy;
mod surface;

use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("guard") => guard(),
        Some("history-check") => history_check(std::env::args().skip(2).collect()),
        Some("surface-check") => surface_check(std::env::args().skip(2).collect()),
        Some("--version" | "-V") => {
            println!("playpilot {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: playpilot guard");
            eprintln!(
                "       playpilot surface-check --range <base>..<head> [--text-env NAME]... [--deny-env NAME]"
            );
            eprintln!(
                "       playpilot history-check --range <base>..<head> [--method squash|merge] [--title <PR title>]"
            );
            ExitCode::from(64)
        }
    }
}

/// `playpilot history-check`: exit 0 passes, exit 1 lists violations.
fn history_check(args: Vec<String>) -> ExitCode {
    let mut range = None;
    let mut title = None;
    let mut method = history::Method::Squash;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--range" => range = it.next(),
            "--title" => title = it.next(),
            "--method" => match it.next().as_deref() {
                Some("squash") => method = history::Method::Squash,
                Some("merge") => method = history::Method::Merge,
                other => {
                    eprintln!("unknown --method {other:?}");
                    return ExitCode::from(64);
                }
            },
            other => {
                eprintln!("unknown argument {other}");
                return ExitCode::from(64);
            }
        }
    }
    let Some(range) = range else {
        eprintln!("--range is required");
        return ExitCode::from(64);
    };
    let commits = match history::read_range(&range) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("history-check: {e}");
            return ExitCode::from(2);
        }
    };
    let problems = history::check(&commits, method, title.as_deref());
    for p in &problems {
        println!("{p}");
    }
    if problems.is_empty() {
        println!("history-check: {} commit(s) ok", commits.len());
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// `playpilot surface-check`: exit 0 passes, exit 1 lists findings.
fn surface_check(args: Vec<String>) -> ExitCode {
    let mut range = None;
    let mut text_envs = Vec::new();
    let mut deny_env = None;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--range" => range = it.next(),
            "--text-env" => text_envs.extend(it.next()),
            "--deny-env" => deny_env = it.next(),
            other => {
                eprintln!("unknown argument {other}");
                return ExitCode::from(64);
            }
        }
    }
    let Some(range) = range else {
        eprintln!("--range is required");
        return ExitCode::from(64);
    };
    let mut items = match surface::read_range(&range) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("surface-check: {e}");
            return ExitCode::from(2);
        }
    };
    for name in &text_envs {
        if let Ok(text) = std::env::var(name) {
            items.push(surface::Item {
                location: name.clone(),
                text,
            });
        }
    }
    let terms: Vec<String> = deny_env
        .and_then(|n| std::env::var(n).ok())
        .map(|v| v.lines().map(str::to_string).collect())
        .unwrap_or_default();
    if terms.iter().all(|t| t.trim().is_empty()) {
        println!("surface-check: no private deny terms configured; generic scans only");
    }
    let findings = surface::scan(&items, &terms);
    for f in &findings {
        println!("{f}");
    }
    if findings.is_empty() {
        println!("surface-check: {} item(s) ok", items.len());
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// `playpilot guard`: exit 0 allows the tool call, exit 2 blocks it.
fn guard() -> ExitCode {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("playpilot guard: could not read the hook input");
        return ExitCode::from(2);
    }
    let policy = policy::Policy::from_env_value(
        std::env::var("PLAYPILOT_PROTECTED_BRANCHES")
            .ok()
            .as_deref(),
    );
    match decide(&policy, &input) {
        Ok(()) => ExitCode::SUCCESS,
        Err(policy::Refusal(msg)) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}

/// Extracts the shell command from a PreToolUse event and checks it.
fn decide(policy: &policy::Policy, input: &str) -> Result<(), policy::Refusal> {
    let event: serde_json::Value = serde_json::from_str(input)
        .map_err(|_| policy::Refusal("playpilot guard: hook input is not JSON".into()))?;
    let tool = event
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !matches!(tool, "Bash" | "PowerShell") {
        return Ok(());
    }
    let command = event
        .pointer("/tool_input/command")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    policy.check_command_line(command)
}

#[cfg(test)]
mod tests;
