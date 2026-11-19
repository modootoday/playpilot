//! Presentation: `playpilot render settings | prompt | node`.
//!
//! `--config` is the fleet configuration repository (holds `fleet/` and
//! `playpilot.lock`) and defaults to the current directory.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use super::output::{self, Action, Mode, Output};
use super::{node, project, prompt};

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

/// `render settings`: writes or checks `<repo>/.claude/settings.json`.
fn settings(a: &Args) -> Result<bool, Usage> {
    let project = a.get("--project").ok_or(Usage)?;
    let repo = Path::new(a.get("--repo").unwrap_or("."));
    report((|| {
        let out = project::settings(a.config(), project, repo)?;
        sync_file(&out, a.check, "render settings")
    })())
}

/// `render prompt`: prints a session's scheduled-task prompt.
fn prompt_cmd(a: &Args) -> Result<bool, Usage> {
    let session = a.get("--session").ok_or(Usage)?;
    report((|| {
        let spec = prompt::session_spec(&super::fleet(a.config())?, session)?;
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
        let runtimes = node::runtimes(&super::fleet(a.config())?, device)?;
        let files = node::node_files(&runtimes, a.get("--workspace"))?;
        let Some(dir) = out else {
            for f in &files {
                print!("==> {} <==\n{}", f.name, f.text);
            }
            return Ok(true);
        };
        let mut ok = true;
        for f in &files {
            let out = Output {
                path: dir.join(f.name),
                text: f.text.clone(),
            };
            ok &= sync_file(&out, a.check, "render node")?;
        }
        Ok(ok)
    })())
}

/// Writes `out` unless it is already there. With `check`, only reports;
/// returns false when the file is stale.
fn sync_file(out: &Output, check: bool, rerun: &str) -> Result<bool, String> {
    let mode = if check { Mode::Check } else { Mode::Write };
    let shown = out.path.display();
    match output::apply(out, mode)? {
        Action::UpToDate => println!("render: {shown} is up to date"),
        Action::Write => println!("render: wrote {shown}"),
        Action::Stale | Action::Refuse => {
            println!("render: {shown} is stale; run `playpilot {rerun}`");
            return Ok(false);
        }
    }
    Ok(true)
}
