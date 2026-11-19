//! `playpilot surface-check`: the `Public surface` required check (common rules C4).
//!
//! Scans what a pull request makes public: added lines, file names, commit
//! messages and the pull request title and body. Findings name the location and
//! the deny term's index, never the term itself (C4.3).

use std::process::Command;

/// One piece of public text and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub location: String,
    pub text: String,
}

/// Returns findings; empty means the check passes.
pub fn scan(items: &[Item], deny_terms: &[String]) -> Vec<String> {
    let terms: Vec<String> = deny_terms
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let mut out = Vec::new();
    for item in items {
        let lower = item.text.to_lowercase();
        for (i, term) in terms.iter().enumerate() {
            if lower.contains(term.as_str()) {
                out.push(format!("{}: matches deny term #{}", item.location, i + 1));
            }
        }
        for email in emails(&item.text) {
            if !allowed_email(&email) {
                out.push(format!("{}: personal email address (C4.6)", item.location));
            }
        }
        if has_local_path(&item.text) {
            out.push(format!("{}: local user path (C4.6)", item.location));
        }
    }
    out
}

/// Addresses that may appear in public text.
fn allowed_email(e: &str) -> bool {
    let e = e.to_ascii_lowercase();
    e.ends_with("@users.noreply.github.com")
        || e == "noreply@github.com"
        || e.ends_with("@example.com")
        || e.ends_with("@example.org")
        || e.ends_with("@example.invalid")
        || e.ends_with("@playpilot.io")
}

fn emails(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || "<>()[]\"'`,;".contains(c)) {
        let w = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if let Some((local, domain)) = w.split_once('@')
            && !local.is_empty()
            && domain.contains('.')
            && !domain.starts_with('.')
            && domain
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            found.push(w.to_string());
        }
    }
    found
}

/// `/home/<name>/`, `/Users/<name>/` or `C:\Users\<name>\` with a real name.
fn has_local_path(text: &str) -> bool {
    for prefix in ["/home/", "/Users/", "C:\\Users\\", "c:\\users\\"] {
        let mut rest = text;
        while let Some(pos) = rest.find(prefix) {
            let after = &rest[pos + prefix.len()..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
                .collect();
            let placeholder = name.is_empty()
                || matches!(
                    name.as_str(),
                    "user" | "runner" | "me" | "you" | "agent" | "ubuntu"
                );
            if !placeholder {
                return true;
            }
            rest = after;
        }
    }
    false
}

/// Collects the public text of `range`: added lines, file names, commit messages.
pub fn read_range(range: &str) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    let diff = git(&["diff", "--unified=0", "--no-color", "--no-ext-diff", range])?;
    let mut file = String::new();
    for line in diff.lines() {
        if let Some(name) = line.strip_prefix("+++ b/") {
            file = name.to_string();
            items.push(Item {
                location: format!("file name {file}"),
                text: file.clone(),
            });
        } else if let Some(added) = line.strip_prefix('+')
            && !line.starts_with("+++")
        {
            items.push(Item {
                location: format!("{file} (added line)"),
                text: added.to_string(),
            });
        }
    }
    let log = git(&["log", "--format=%h%x1f%B%x1e", range])?;
    for rec in log.split('\u{1e}') {
        if let Some((sha, msg)) = rec.trim_start().split_once('\u{1f}') {
            items.push(Item {
                location: format!("commit {sha} message"),
                text: msg.to_string(),
            });
        }
    }
    Ok(items)
}

fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
