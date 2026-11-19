//! Presentation: `playpilot node install`, puts this node's service files
//! from `render node` into their OS locations.
//!
//! It only writes files. Enabling the service is left to the owner: the
//! commands are printed, never run, and credentials are never touched.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::render::node::{self, Runtime};
use crate::render::output::{self, Action, Mode, Output};

pub const USAGE: &str = "usage: playpilot node install --device <name> [--config <fleet repo>] [--workspace <dir>] [--dry-run] [--force]";

/// The operating system whose service manager receives the files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// systemd user units; WSL distributions are Linux.
    Linux,
    /// Task Scheduler.
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

/// Parsed `node install` arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub device: String,
    pub config: String,
    pub workspace: Option<String>,
    pub dry_run: bool,
    pub force: bool,
}

/// Parses the arguments after `node install`; `None` is a usage error.
pub fn parse(args: Vec<String>) -> Option<Args> {
    let (mut device, mut config, mut workspace) = (None, None, None);
    let (mut dry_run, mut force) = (false, false);
    let mut it = args.into_iter();
    while let Some(x) = it.next() {
        let slot = match x.as_str() {
            "--dry-run" if !dry_run => {
                dry_run = true;
                continue;
            }
            "--force" if !force => {
                force = true;
                continue;
            }
            "--device" => &mut device,
            "--config" => &mut config,
            "--workspace" => &mut workspace,
            _ => return None,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(it.next()?);
    }
    Some(Args {
        device: device?,
        config: config.unwrap_or_else(|| ".".into()),
        workspace,
        dry_run,
        force,
    })
}

/// The device's runtimes that this platform serves.
pub fn select(runtimes: &[Runtime], platform: Platform) -> Result<Vec<Runtime>, String> {
    let picked: Vec<Runtime> = runtimes
        .iter()
        .copied()
        .filter(|r| match platform {
            Platform::Linux => matches!(r, Runtime::Wsl { .. }),
            Platform::Windows => matches!(r, Runtime::Windows { .. }),
        })
        .collect();
    if picked.is_empty() {
        let want = match platform {
            Platform::Linux => "wsl",
            Platform::Windows => "windows",
        };
        return Err(format!(
            "the device lists no `{want}` runtime for this platform"
        ));
    }
    Ok(picked)
}

/// Where the service files go: `$XDG_CONFIG_HOME/systemd/user` (default
/// `$HOME/.config`) on Linux, `%APPDATA%\fleet` on Windows.
pub fn install_dir(
    platform: Platform,
    env: impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, String> {
    let set = |k: &str| env(k).filter(|v| !v.is_empty());
    match platform {
        Platform::Linux => {
            let config = match set("XDG_CONFIG_HOME").filter(|v| Path::new(v).is_absolute()) {
                Some(x) => PathBuf::from(x),
                None => PathBuf::from(set("HOME").ok_or("HOME is not set")?).join(".config"),
            };
            Ok(config.join("systemd").join("user"))
        }
        Platform::Windows => {
            Ok(PathBuf::from(set("APPDATA").ok_or("APPDATA is not set")?).join("fleet"))
        }
    }
}

/// The commands the owner runs after the files are in place.
pub fn next_steps(platform: Platform, files: &[Output], workspace: Option<&str>) -> Vec<String> {
    match platform {
        Platform::Linux => vec![
            "systemctl --user daemon-reload".into(),
            format!(
                "systemctl --user enable --now claude-rc@{}",
                workspace.unwrap_or("<dir under ~/fleet>")
            ),
        ],
        Platform::Windows => files
            .iter()
            .map(|f| {
                let path = f.path.display().to_string().replace('\'', "''");
                format!(
                    "Register-ScheduledTask -TaskName claude-rc -Xml (Get-Content -Raw '{path}')"
                )
            })
            .collect(),
    }
}

/// The files to install for `runtimes` under `dir`.
pub fn targets(
    runtimes: &[Runtime],
    workspace: Option<&str>,
    dir: &Path,
) -> Result<Vec<Output>, String> {
    Ok(node::node_files(runtimes, workspace)?
        .into_iter()
        .map(|f| Output {
            path: dir.join(f.name),
            text: f.text,
        })
        .collect())
}

pub fn main(args: Vec<String>) -> ExitCode {
    let Some(a) = parse(args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    };
    match run(&a, Platform::current()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("node install: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(a: &Args, platform: Platform) -> Result<bool, String> {
    let workspace = a
        .workspace
        .as_deref()
        .map(node::plain_workspace)
        .transpose()?;
    let all = node::runtimes(&crate::render::fleet(Path::new(&a.config))?, &a.device)?;
    let runtimes = select(&all, platform)?;
    let dir = install_dir(platform, |k| std::env::var(k).ok())?;
    let files = targets(&runtimes, workspace, &dir)?;
    let mode = Mode::Install { force: a.force };
    let mut ok = true;
    for f in &files {
        let shown = f.path.display();
        if a.dry_run {
            let existing = output::read(&f.path)?;
            let note = match output::decide(existing.as_deref(), &f.text, mode) {
                Action::UpToDate => "is up to date".to_string(),
                Action::Write => "would be written".to_string(),
                Action::Stale | Action::Refuse => {
                    "differs and would be refused without --force".to_string()
                }
            };
            println!("node install: {shown} {note}");
            print!("==> {shown} <==\n{}", f.text);
            continue;
        }
        match output::apply(f, mode)? {
            Action::UpToDate => println!("node install: {shown} is up to date"),
            Action::Write => println!("node install: wrote {shown}"),
            Action::Stale | Action::Refuse => {
                println!("node install: {shown} differs; left as is (pass --force to replace)");
                ok = false;
            }
        }
    }
    if ok {
        println!("node install: run these yourself to enable the service:");
        for step in next_steps(platform, &files, workspace) {
            println!("  {step}");
        }
    }
    Ok(ok)
}
