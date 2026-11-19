//! `playpilot guard`: a Claude Code PreToolUse hook.
//!
//! Reads the hook event JSON on stdin. Exit 0 allows the tool call; exit 2
//! blocks it and the stderr text is shown to Claude.

mod history;
mod mail;
mod mcp;
mod policy;
mod render;
mod surface;
mod validate;

use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("guard") => guard(),
        Some("mail") => mail::cli::main(std::env::args().skip(2).collect()),
        Some("mcp") => mcp::main(),
        Some("history-check") => history_check(std::env::args().skip(2).collect()),
        Some("surface-check") => surface_check(std::env::args().skip(2).collect()),
        Some("validate") => validate_dir(std::env::args().nth(2)),
        Some("render") => render_cmd(std::env::args().skip(2).collect()),
        Some("--version" | "-V") => {
            println!("playpilot {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: playpilot guard");
            eprintln!("       playpilot mail <send|inbox|read|ack> ... (playpilot mail --help)");
            eprintln!(
                "       playpilot mcp   (MCP server on stdio; env PLAYPILOT_MAILBOX_REPO, PLAYPILOT_ACTOR)"
            );
            eprintln!("       playpilot validate <fleet-dir>");
            eprintln!("       {RENDER_USAGE}");
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

/// `playpilot validate <dir>`: exit 0 valid, exit 1 lists problems.
fn validate_dir(dir: Option<String>) -> ExitCode {
    let Some(dir) = dir else {
        eprintln!("usage: playpilot validate <fleet-dir>");
        return ExitCode::from(64);
    };
    let problems = match validate::read_dir(std::path::Path::new(&dir)) {
        Ok(resources) => {
            let p = validate::check(&resources);
            if p.is_empty() {
                println!("validate: {} resource(s) ok", resources.len());
            }
            p
        }
        Err(errors) => errors,
    };
    for p in &problems {
        println!("{p}");
    }
    if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

const RENDER_USAGE: &str =
    "playpilot render settings --project <name> [--config <dir>] [--repo <dir>] [--check]";

/// `playpilot render settings`: writes or checks `<repo>/.claude/settings.json`.
/// `--config` is the fleet configuration repository (holds `fleet/` and
/// `playpilot.lock`); both directories default to the current one.
fn render_cmd(args: Vec<String>) -> ExitCode {
    let mut it = args.into_iter();
    if it.next().as_deref() != Some("settings") {
        eprintln!("usage: {RENDER_USAGE}");
        return ExitCode::from(64);
    }
    let (mut project, mut config, mut repo, mut check) =
        (None, ".".to_string(), ".".to_string(), false);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--project" => project = it.next(),
            "--config" => config = it.next().unwrap_or_default(),
            "--repo" => repo = it.next().unwrap_or_default(),
            "--check" => check = true,
            _ => {
                eprintln!("usage: {RENDER_USAGE}");
                return ExitCode::from(64);
            }
        }
    }
    let Some(project) = project else {
        eprintln!("usage: {RENDER_USAGE}");
        return ExitCode::from(64);
    };
    let config = std::path::Path::new(&config);
    let target = std::path::Path::new(&repo)
        .join(".claude")
        .join("settings.json");
    let result = (|| -> Result<(String, Option<String>), String> {
        let lock = std::fs::read_to_string(config.join("playpilot.lock"))
            .map_err(|e| format!("{}: {e}", config.join("playpilot.lock").display()))?;
        let rev = render::lock_rev(&lock)?;
        let resources = validate::read_dir(&config.join("fleet")).map_err(|e| e.join("\n"))?;
        let spec = render::guard_spec(&resources, &project, rev)?;
        let existing = std::fs::read_to_string(&target).ok();
        let text = render::merge_settings(existing.as_deref(), &spec)?;
        Ok((text, existing))
    })();
    let (text, existing) = match result {
        Ok(v) => v,
        Err(e) => {
            eprintln!("render: {e}");
            return ExitCode::from(1);
        }
    };
    if existing.as_deref() == Some(text.as_str()) {
        println!("render: {} is up to date", target.display());
        return ExitCode::SUCCESS;
    }
    if check {
        println!(
            "render: {} is stale; run `playpilot render settings`",
            target.display()
        );
        return ExitCode::from(1);
    }
    if let Some(dir) = target.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("render: {}: {e}", dir.display());
        return ExitCode::from(1);
    }
    match std::fs::write(&target, text) {
        Ok(()) => {
            println!("render: wrote {}", target.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("render: {}: {e}", target.display());
            ExitCode::from(1)
        }
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
    let mut policy = policy::Policy::from_env_value(
        std::env::var("PLAYPILOT_PROTECTED_BRANCHES")
            .ok()
            .as_deref(),
    );
    policy.local_tags = local_tags(&input);
    match decide(&policy, &input) {
        Ok(()) => ExitCode::SUCCESS,
        Err(policy::Refusal(msg)) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}

/// Tags of the repository at the hook's `cwd`; empty when git is unavailable.
fn local_tags(input: &str) -> Vec<String> {
    let cwd = serde_json::from_str::<serde_json::Value>(input)
        .ok()
        .and_then(|e| e.get("cwd").and_then(|v| v.as_str()).map(String::from));
    let mut git = std::process::Command::new("git");
    git.args(["tag", "--list"]);
    if let Some(dir) = cwd {
        git.current_dir(dir);
    }
    match git.output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
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
