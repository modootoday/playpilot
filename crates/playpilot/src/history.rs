//! `playpilot history-check`: the `History` required check (commit-history H6).

use std::process::Command;

/// Commit types allowed in a Conventional Commits subject.
const TYPES: [&str; 11] = [
    "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert",
];

/// Committer that GitHub itself uses for merges and web edits (common rule C10.5).
const WEB_FLOW: &str = "noreply@github.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub parents: usize,
    pub author: String,
    pub committer: String,
    pub subject: String,
    pub body: String,
}

/// How the pull request will land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Squash,
    Merge,
}

/// Returns every rule violation found; empty means the check passes.
pub fn check(commits: &[Commit], method: Method, title: Option<&str>) -> Vec<String> {
    let mut problems = Vec::new();
    if method == Method::Squash {
        match title {
            Some(t) => {
                if let Err(e) = conventional(t) {
                    problems.push(format!("pull request title: {e}"));
                }
            }
            None => {
                problems.push("squash repositories need --title \"<pull request title>\"".into())
            }
        }
    }
    for c in commits {
        let short = &c.sha[..c.sha.len().min(10)];
        if c.parents > 1 {
            problems.push(format!(
                "{short}: merge commit in the topic branch; rebase instead (H3)"
            ));
        }
        let lower = c.subject.to_ascii_lowercase();
        for marker in ["fixup!", "squash!", "amend!", "wip"] {
            if lower.starts_with(marker) {
                problems.push(format!(
                    "{short}: `{marker}` commit must be folded before merge (H3)"
                ));
            }
        }
        if method == Method::Merge
            && let Err(e) = conventional(&c.subject)
        {
            problems.push(format!("{short}: {e}"));
        }
        if !c.author.ends_with("@users.noreply.github.com") {
            problems.push(format!(
                "{short}: author must be the owner noreply identity (C10.5)"
            ));
        }
        if !(c.committer.ends_with("@users.noreply.github.com") || c.committer == WEB_FLOW) {
            problems.push(format!(
                "{short}: committer must be the owner noreply identity (C10.5)"
            ));
        }
        for line in c.body.lines() {
            let l = line.trim().to_ascii_lowercase();
            if let Some(rest) = l.strip_prefix("co-authored-by:")
                && !rest
                    .trim_end_matches('>')
                    .trim()
                    .ends_with("@users.noreply.github.com")
            {
                problems.push(format!(
                    "{short}: Co-authored-by must use a noreply address (C10.2)"
                ));
            }
            if l.starts_with("claude-session:") {
                problems.push(format!(
                    "{short}: session links belong in the pull request body (C10.3)"
                ));
            }
        }
    }
    problems
}

/// Validates `type(scope)!: summary`.
pub fn conventional(subject: &str) -> Result<(), String> {
    let Some((head, summary)) = subject.split_once(": ") else {
        return Err(format!("`{subject}` is not `type(scope): summary`"));
    };
    let head = head.strip_suffix('!').unwrap_or(head);
    let kind = match head.split_once('(') {
        Some((k, scope)) if scope.ends_with(')') && scope.len() > 1 => k,
        Some(_) => return Err(format!("`{subject}` has a malformed scope")),
        None => head,
    };
    if !TYPES.contains(&kind) {
        return Err(format!("`{kind}` is not one of {}", TYPES.join(", ")));
    }
    if summary.trim().is_empty() {
        return Err("empty summary".into());
    }
    if subject.chars().count() > 72 {
        return Err(format!(
            "subject is {} characters; the limit is 72",
            subject.chars().count()
        ));
    }
    Ok(())
}

/// Reads the commits of `range` (e.g. `origin/main..HEAD`) with git.
pub fn read_range(range: &str) -> Result<Vec<Commit>, String> {
    let out = Command::new("git")
        .args([
            "log",
            "--format=%H%x1f%P%x1f%ae%x1f%ce%x1f%s%x1f%b%x1e",
            range,
        ])
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(parse_log(&String::from_utf8_lossy(&out.stdout)))
}

/// Parses the record-separated output of [`read_range`].
pub fn parse_log(text: &str) -> Vec<Commit> {
    text.split('\u{1e}')
        .filter_map(|rec| {
            let f: Vec<&str> = rec.trim_start_matches('\n').split('\u{1f}').collect();
            (f.len() == 6 && !f[0].is_empty()).then(|| Commit {
                sha: f[0].into(),
                parents: f[1].split_whitespace().count(),
                author: f[2].into(),
                committer: f[3].into(),
                subject: f[4].into(),
                body: f[5].into(),
            })
        })
        .collect()
}
