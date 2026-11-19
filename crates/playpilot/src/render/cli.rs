//! Presentation: `playpilot render settings | prompt | node`.
//!
//! `--config` is the fleet configuration repository (holds `fleet/` and
//! `playpilot.lock`) and defaults to the current directory.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use super::{node, prompt, settings as guard};
use crate::validate::{self, Resource};

pub const USAGE: &str = "\
usage: playpilot render settings --project <name> [--config <dir>] [--repo <dir>] [--check]
       playpilot render prompt   --session <name> [--config <dir>]
       playpilot render node     --device <name> [--workspace <dir>] [--config <dir>] [--out <dir> [--check]]";

/// Value options each verb accepts; `--check` is the only flag.
fn options(verb: &str) -> Option<&'static [&'static str]> {
    match verb {
        "settings" => Some(&["--project", "--config", "--repo"]),
        "prompt" => Some(&["--session", "--config"]),
        "node" => Some(&["--device", "--workspace", "--config", "--out"]),
        _ => None,
    }
}

struct Args {
    verb: String,
    values: BTreeMap<String, String>,
    check: bool,
}

impl Args {
    fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    fn config(&self) -> &Path {
        Path::new(self.get("--config").unwrap_or("."))
    }
}

fn parse(args: Vec<String>) -> Option<Args> {
    let mut it = args.into_iter();
    let verb = it.next()?;
    let known = options(&verb)?;
    let mut a = Args {
        verb,
        values: BTreeMap::new(),
        check: false,
    };
    while let Some(x) = it.next() {
        if x == "--check" && a.verb != "prompt" {
            a.check = true;
        } else if known.contains(&x.as_str()) {
            a.values.insert(x, it.next()?);
        } else {
            return None;
        }
    }
    Some(a)
}

pub fn main(args: Vec<String>) -> ExitCode {
    let Some(a) = parse(args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    };
    let result = match a.verb.as_str() {
        "settings" => settings(&a),
        "prompt" => prompt_cmd(&a),
        _ => node_cmd(&a),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(Usage) => {
            eprintln!("{USAGE}");
            ExitCode::from(64)
        }
    }
}

/// A missing required option; the caller prints the usage.
struct Usage;

fn report(result: Result<bool, String>) -> Result<bool, Usage> {
    result.or_else(|e| {
        eprintln!("render: {e}");
        Ok(false)
    })
}

fn fleet(config: &Path) -> Result<Vec<Resource>, String> {
    validate::read_dir(&config.join("fleet")).map_err(|e| e.join("\n"))
}

/// `render settings`: writes or checks `<repo>/.claude/settings.json`.
fn settings(a: &Args) -> Result<bool, Usage> {
    let project = a.get("--project").ok_or(Usage)?;
    let target = Path::new(a.get("--repo").unwrap_or("."))
        .join(".claude")
        .join("settings.json");
    report((|| {
        let lock_path = a.config().join("playpilot.lock");
        let lock = std::fs::read_to_string(&lock_path)
            .map_err(|e| format!("{}: {e}", lock_path.display()))?;
        let rev = guard::lock_rev(&lock)?;
        let spec = guard::guard_spec(&fleet(a.config())?, project, rev)?;
        let existing = std::fs::read_to_string(&target).ok();
        let text = guard::merge_settings(existing.as_deref(), &spec)?;
        sync_file(&target, &text, a.check, "render settings")
    })())
}

/// `render prompt`: prints a session's scheduled-task prompt.
fn prompt_cmd(a: &Args) -> Result<bool, Usage> {
    let session = a.get("--session").ok_or(Usage)?;
    report((|| {
        let spec = prompt::session_spec(&fleet(a.config())?, session)?;
        print!("{}", prompt::prompt_text(&spec));
        Ok(true)
    })())
}

/// `render node`: prints, writes or checks a device's service files.
fn node_cmd(a: &Args) -> Result<bool, Usage> {
    let device = a.get("--device").ok_or(Usage)?;
    let out = a.get("--out").map(Path::new);
    if a.check && out.is_none() {
        return Err(Usage);
    }
    report((|| {
        let runtimes = node::runtimes(&fleet(a.config())?, device)?;
        let files = node::node_files(&runtimes, a.get("--workspace"))?;
        let Some(dir) = out else {
            for f in &files {
                print!("==> {} <==\n{}", f.name, f.text);
            }
            return Ok(true);
        };
        let mut ok = true;
        for f in &files {
            ok &= sync_file(&dir.join(f.name), &f.text, a.check, "render node")?;
        }
        Ok(ok)
    })())
}

/// Writes `text` to `target` unless it is already there. With `check`, only
/// reports; returns false when the file is stale.
fn sync_file(target: &Path, text: &str, check: bool, rerun: &str) -> Result<bool, String> {
    if std::fs::read_to_string(target).ok().as_deref() == Some(text) {
        println!("render: {} is up to date", target.display());
        return Ok(true);
    }
    if check {
        println!(
            "render: {} is stale; run `playpilot {rerun}`",
            target.display()
        );
        return Ok(false);
    }
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(target, text).map_err(|e| format!("{}: {e}", target.display()))?;
    println!("render: wrote {}", target.display());
    Ok(true)
}
