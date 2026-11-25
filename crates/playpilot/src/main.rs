//! `playpilot guard`: a Claude Code PreToolUse hook.
//!
//! Reads the hook event JSON on stdin. Exit 0 allows the tool call; exit 2
//! blocks it and the stderr text is shown to Claude.

mod claim;
mod events;
mod gh;
mod history;
mod install;
mod mail;
mod mcp;
mod migrate;
mod plan;
mod plugin_gate;
mod policy;
mod render;
mod status;
mod surface;
mod sync;
mod validate;

use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("guard") => guard(),
        Some("mail") => mail::cli::main(std::env::args().skip(2).collect()),
        Some("events") => events::cli::main(std::env::args().skip(2).collect()),
        Some("mcp") => mcp::main(),
        Some(v @ ("claim" | "release")) => claim::cli::main(v, std::env::args().skip(2).collect()),
        Some("history-check") => history_check(std::env::args().skip(2).collect()),
        Some("surface-check") => surface_check(std::env::args().skip(2).collect()),
        Some("validate") => validate_dir(std::env::args().nth(2)),
        Some("migrate") => migrate::cli::main(std::env::args().skip(2).collect()),
        Some("render") => render::cli::main(std::env::args().skip(2).collect()),
        Some("status") => status::main(std::env::args().skip(2).collect()),
        Some("sync") => sync::main(std::env::args().skip(2).collect()),
        Some("node") if std::env::args().nth(2).as_deref() == Some("install") => {
            install::main(std::env::args().skip(3).collect())
        }
        Some(v @ ("plan" | "whoami")) => plan_cmd(v, std::env::args().skip(2).collect()),
        Some("--version" | "-V") => {
            println!("playpilot {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: playpilot guard");
            eprintln!("       playpilot mail <send|inbox|read|ack> ... (playpilot mail --help)");
            eprintln!(
                "       playpilot mcp   (MCP server on stdio; env PLAYPILOT_MAIL_URL or PLAYPILOT_MAILBOX_REPO, PLAYPILOT_ACTOR)"
            );
            eprintln!("       playpilot claim|release <owner/repo#n> ... (playpilot claim --help)");
            eprintln!(
                "       {}",
                events::cli::USAGE
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim_start_matches("usage: ")
            );
            eprintln!("       playpilot validate <fleet-dir>");
            eprintln!(
                "       {}",
                migrate::cli::USAGE.trim_start_matches("usage: ")
            );
            eprintln!("       {PLAN_USAGE}");
            eprintln!(
                "       playpilot render <settings|roles|prompt|node|plugins> ... (playpilot render --help)"
            );
            eprintln!("       {}", status::USAGE.trim_start_matches("usage: "));
            eprintln!("       {}", sync::USAGE.trim_start_matches("usage: "));
            eprintln!("       {}", install::USAGE.trim_start_matches("usage: "));
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

const PLAN_USAGE: &str =
    "playpilot plan [--config <dir>] | playpilot whoami --device <name> [--config <dir>]";

/// `playpilot plan` / `whoami`: read-only views of the fleet configuration.
/// `--config` holds `fleet/` (default: current directory); the device defaults
/// to `PLAYPILOT_DEVICE`.
fn plan_cmd(verb: &str, args: Vec<String>) -> ExitCode {
    let (mut config, mut device) = (".".to_string(), std::env::var("PLAYPILOT_DEVICE").ok());
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--config", Some(v)) => config = v,
            ("--device", Some(v)) if verb == "whoami" => device = Some(v),
            _ => {
                eprintln!("usage: {PLAN_USAGE}");
                return ExitCode::from(64);
            }
        }
    }
    let resources = match validate::read_dir(&std::path::Path::new(&config).join("fleet")) {
        Ok(r) => r,
        Err(e) => {
            for p in e {
                eprintln!("{verb}: {p}");
            }
            return ExitCode::from(1);
        }
    };
    let out = if verb == "plan" {
        Ok(plan::plan(&resources))
    } else {
        match device {
            Some(d) => plan::whoami(&resources, &d),
            None => Err("set --device or PLAYPILOT_DEVICE".into()),
        }
    };
    match out {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{verb}: {e}");
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
    let plugins = std::env::var_os(plugin_gate::ENV);
    match decide_with_plugins(&policy, &input, plugins.as_deref()) {
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

/// [`decide_with_plugins`] without policy plugins.
#[cfg(test)]
fn decide(policy: &policy::Policy, input: &str) -> Result<(), policy::Refusal> {
    decide_with_plugins(policy, input, None)
}

/// Extracts the shell command from a PreToolUse event and checks it: the
/// core policy first; policy plugins (`PLAYPILOT_POLICY_PLUGINS`) only
/// see commands the core allowed, so they can refuse but never allow.
fn decide_with_plugins(
    policy: &policy::Policy,
    input: &str,
    plugins: Option<&std::ffi::OsStr>,
) -> Result<(), policy::Refusal> {
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
    policy.check_command_line(command)?;
    plugin_gate::check(plugins, tool, command)
}

#[cfg(test)]
mod tests;
