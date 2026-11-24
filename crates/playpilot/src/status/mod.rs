//! Presentation: `playpilot status --from <url|file> [--out <path>]`, a
//! validated snapshot of the Worker's public `/v1/state` for the status page.
//!
//! Exit 0 when the snapshot was written, 1 when it could not be read or holds
//! a field outside the public subset, 64 on usage errors.

pub mod snapshot;

use std::io::Write;
use std::process::{Command, ExitCode, Stdio};

use crate::mail::worker::quote;

pub const USAGE: &str = "usage: playpilot status --from <url|file> [--out <path>]";
const MAX_BYTES: usize = 1 << 20;

/// Parsed `status` arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub from: String,
    pub out: Option<String>,
}

/// Parses the arguments after `status`; `None` is a usage error.
pub fn parse(args: Vec<String>) -> Option<Args> {
    let (mut from, mut out) = (None, None);
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let slot = match a.as_str() {
            "--from" => &mut from,
            "--out" => &mut out,
            _ => return None,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(it.next()?);
    }
    Some(Args { from: from?, out })
}

pub fn main(args: Vec<String>) -> ExitCode {
    let Some(args) = parse(args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    };
    let text = match read(&args.from) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("status: {e}");
            return ExitCode::from(1);
        }
    };
    let snapshot = match snapshot::filter(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("status: {e}");
            return ExitCode::from(1);
        }
    };
    let body = serde_json::to_string_pretty(&snapshot).unwrap_or_default() + "\n";
    match &args.out {
        None => {
            print!("{body}");
            ExitCode::SUCCESS
        }
        Some(path) => match std::fs::write(path, body) {
            Ok(()) => {
                println!("status: wrote {path}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("status: could not write {path}: {e}");
                ExitCode::from(1)
            }
        },
    }
}

fn is_url(from: &str) -> bool {
    from.starts_with("https://") || from.starts_with("http://")
}

fn read(from: &str) -> Result<String, String> {
    let text = if is_url(from) {
        fetch(from)?
    } else {
        std::fs::read_to_string(from).map_err(|e| format!("could not read {from}: {e}"))?
    };
    if text.len() > MAX_BYTES {
        return Err(format!("the state is larger than {MAX_BYTES} bytes"));
    }
    Ok(text)
}

/// The curl config (`curl --config -`) for one GET of the public state.
pub fn curl_config(url: &str) -> String {
    [
        format!("url = {}", quote(url)),
        "header = \"Accept: application/json\"".to_string(),
        "max-time = 30".to_string(),
        format!("max-filesize = {MAX_BYTES}"),
        "fail".to_string(),
        "proto = \"=http,https\"".to_string(),
    ]
    .join("\n")
        + "\n"
}

/// `curl`, as the Worker mail transport runs it; the public state needs no credential.
fn fetch(url: &str) -> Result<String, String> {
    let mut child = Command::new("curl")
        .args(["-sS", "--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if let Some(mut pipe) = child.stdin.take() {
        pipe.write_all(curl_config(url).as_bytes())
            .map_err(|e| format!("could not write to curl: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("curl failed: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "curl failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|_| "the state is not UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_arguments() {
        assert_eq!(
            parse(s(&["--from", "state.json", "--out", "docs/status.json"])),
            Some(Args {
                from: "state.json".into(),
                out: Some("docs/status.json".into())
            })
        );
        assert_eq!(parse(s(&["--from", "x"])).unwrap().out, None);
        assert_eq!(parse(s(&[])), None);
        assert_eq!(parse(s(&["--out", "x"])), None);
        assert_eq!(parse(s(&["--from"])), None);
        assert_eq!(parse(s(&["--from", "a", "--from", "b"])), None);
        assert_eq!(parse(s(&["--from", "a", "--bogus"])), None);
    }

    #[test]
    fn curl_gets_without_credentials() {
        let c = curl_config("https://api.example.com/v1/state");
        assert!(c.contains("url = \"https://api.example.com/v1/state\""));
        assert!(c.contains("\nfail\n"));
        assert!(!c.contains("Authorization"));
        assert!(!c.contains("request"));
        assert!(is_url("https://api.example.com/v1/state"));
        assert!(!is_url("state.json"));
    }
}
