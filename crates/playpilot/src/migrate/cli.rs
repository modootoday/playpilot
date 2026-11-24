//! Presentation: `playpilot migrate [--check] [--force] [--to <apiVersion>] <fleet-dir>`.
//!
//! Exit 0 when nothing changes (or every file was written), 1 when a file
//! would change under `--check`, an apiVersion is unknown or newer, the result
//! does not validate, or a rewrite would drop comments without `--force`;
//! 64 on usage errors.

use std::path::Path;
use std::process::ExitCode;

use super::{Outcome, Registry, migrate_text};
use crate::validate;

pub const USAGE: &str =
    "usage: playpilot migrate [--check] [--force] [--to <apiVersion>] <fleet-dir>";

/// Parsed `migrate` arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub dir: String,
    pub to: Option<String>,
    pub check: bool,
    pub force: bool,
}

/// Parses the arguments after `migrate`; `None` is a usage error.
pub fn parse(args: Vec<String>) -> Option<Args> {
    let (mut dir, mut to, mut check, mut force) = (None, None, false, false);
    let mut it = args.into_iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--check" if !check => check = true,
            "--force" if !force => force = true,
            "--to" if to.is_none() => to = Some(it.next()?),
            s if !s.starts_with('-') && dir.is_none() => dir = Some(x),
            _ => return None,
        }
    }
    Some(Args {
        dir: dir?,
        to,
        check,
        force,
    })
}

pub fn main(args: Vec<String>) -> ExitCode {
    let Some(a) = parse(args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    };
    match run(&a, &Registry::builtin()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(problems) => {
            for p in problems {
                eprintln!("migrate: {p}");
            }
            ExitCode::from(1)
        }
    }
}

struct Planned {
    file: String,
    outcome: Outcome,
}

pub(crate) fn run(a: &Args, registry: &Registry) -> Result<bool, Vec<String>> {
    let to =
        a.to.clone()
            .unwrap_or_else(|| registry.newest().to_string());
    if !registry.versions.contains(&to.as_str()) {
        let known = registry.versions.join(", ");
        return Err(vec![format!(
            "unknown target apiVersion \"{to}\" (known: {known})"
        )]);
    }
    let files =
        validate::toml_files(Path::new(&a.dir)).map_err(|e| vec![format!("{}: {e}", a.dir)])?;
    let mut planned = Vec::new();
    let mut problems = Vec::new();
    let mut resources = Vec::new();
    for path in files {
        let file = path.display().to_string();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                problems.push(format!("{file}: {e}"));
                continue;
            }
        };
        match migrate_text(registry, &file, &text, &to) {
            Ok(outcome) => {
                let result = match &outcome {
                    Outcome::Unchanged => &text,
                    Outcome::Rewrite { text, .. } => text,
                };
                if to == validate::API_VERSION {
                    match validate::parse(&file, result) {
                        Ok(r) => resources.push(r),
                        Err(e) => problems.push(e),
                    }
                }
                planned.push(Planned { file, outcome });
            }
            Err(e) => problems.push(e),
        }
    }
    if problems.is_empty() {
        problems.extend(validate::check(&resources));
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let changes: Vec<&Planned> = planned
        .iter()
        .filter(|p| p.outcome != Outcome::Unchanged)
        .collect();
    if changes.is_empty() {
        println!(
            "migrate: {} resource(s) already at {to}; nothing to change",
            planned.len()
        );
        return Ok(true);
    }
    let lossy: Vec<&str> = changes
        .iter()
        .filter(|p| matches!(p.outcome, Outcome::Rewrite { lossy: true, .. }))
        .map(|p| p.file.as_str())
        .collect();
    if a.check {
        for p in &changes {
            if let Outcome::Rewrite { from, lossy, .. } = &p.outcome {
                let note = if *lossy { " (drops comments)" } else { "" };
                println!("migrate: {} would change ({from} -> {to}){note}", p.file);
            }
        }
        println!(
            "migrate: {} of {} file(s) would change; run `playpilot migrate {}`",
            changes.len(),
            planned.len(),
            a.dir
        );
        return Ok(false);
    }
    if !lossy.is_empty() && !a.force {
        for f in &lossy {
            println!("migrate: {f}: the rewrite cannot keep its TOML comments");
        }
        println!("migrate: nothing written; rerun with --force to write without those comments");
        return Ok(false);
    }
    for p in &changes {
        if let Outcome::Rewrite { from, text, .. } = &p.outcome {
            std::fs::write(&p.file, text).map_err(|e| vec![format!("{}: {e}", p.file)])?;
            println!("migrate: wrote {} ({from} -> {to})", p.file);
        }
    }
    println!(
        "migrate: {} of {} file(s) rewritten",
        changes.len(),
        planned.len()
    );
    Ok(true)
}
