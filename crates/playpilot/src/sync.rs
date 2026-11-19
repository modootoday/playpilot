//! Presentation: `playpilot sync [--check]`, every project-side file `render`
//! produces for one checkout, checked or written in one run.
//!
//! Exit 0 when every file matches (or was written), 1 when a file is stale or
//! the configuration cannot be read, 64 on usage errors.

use std::path::Path;
use std::process::ExitCode;

use crate::render::output::{self, Action, Mode};
use crate::render::project;

pub const USAGE: &str = "usage: playpilot sync [--check] --project <name> [--config <fleet repo>] [--repo <project checkout>]";

/// Parsed `sync` arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub project: String,
    pub config: String,
    pub repo: String,
    pub check: bool,
}

/// Parses the arguments after `sync`; `None` is a usage error.
pub fn parse(args: Vec<String>) -> Option<Args> {
    let (mut project, mut config, mut repo, mut check) = (None, None, None, false);
    let mut it = args.into_iter();
    while let Some(x) = it.next() {
        let slot = match x.as_str() {
            "--check" if !check => {
                check = true;
                continue;
            }
            "--project" => &mut project,
            "--config" => &mut config,
            "--repo" => &mut repo,
            _ => return None,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(it.next()?);
    }
    Some(Args {
        project: project?,
        config: config.unwrap_or_else(|| ".".into()),
        repo: repo.unwrap_or_else(|| ".".into()),
        check,
    })
}

pub fn main(args: Vec<String>) -> ExitCode {
    let Some(a) = parse(args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    };
    match run(&a) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("sync: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(a: &Args) -> Result<bool, String> {
    let outputs = project::outputs(Path::new(&a.config), &a.project, Path::new(&a.repo))?;
    let mode = if a.check { Mode::Check } else { Mode::Write };
    let mut stale = Vec::new();
    for out in &outputs {
        match output::apply(out, mode)? {
            Action::UpToDate => {}
            Action::Write => println!("sync: wrote {}", out.path.display()),
            Action::Stale | Action::Refuse => stale.push(out.path.display().to_string()),
        }
    }
    for path in &stale {
        println!("sync: {path} is stale");
    }
    if stale.is_empty() {
        println!("sync: {} file(s) up to date", outputs.len());
        Ok(true)
    } else {
        println!(
            "sync: {} of {} file(s) stale; run `playpilot sync --project {}`",
            stale.len(),
            outputs.len(),
            a.project
        );
        Ok(false)
    }
}
