//! Policy: decides whether one shell command line may run.
//!
//! The policy is deliberately small and mirrors the common automation rules:
//! no force push, no push to a protected branch, no tag or release, no
//! direct merge or approval, explicit staging only, and no attribution
//! trailers other than `@users.noreply.github.com` co-authors.

/// Why a command was refused. The text is shown to the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

/// Provider prefixes of agent topic branches (common rule C1.2).
const AGENT_PREFIXES: [&str; 6] = [
    "claude/", "codex/", "cursor/", "grok/", "gemini/", "copilot/",
];

/// Branches nobody pushes to directly.
#[derive(Debug, Clone)]
pub struct Policy {
    pub protected_branches: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            protected_branches: vec!["main".into(), "master".into()],
        }
    }
}

impl Policy {
    /// Reads `PLAYPILOT_PROTECTED_BRANCHES` (comma separated) when it is set.
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value {
            Some(v) if !v.trim().is_empty() => Self {
                protected_branches: v
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
            },
            _ => Self::default(),
        }
    }

    /// Checks a whole command line, which may hold several commands.
    pub fn check_command_line(&self, line: &str) -> Result<(), Refusal> {
        if let Some(r) = check_trailers(line) {
            return Err(r);
        }
        let (line, shell_bodies) = take_heredocs(line);
        for body in shell_bodies {
            self.check_command_line(&body)?;
        }
        for segment in split_segments(&line) {
            let Some(words) = shlex::split(&segment) else {
                // Unbalanced quotes: refuse rather than guess.
                return Err(Refusal(format!(
                    "playpilot guard could not parse `{segment}`; rewrite it without unbalanced quotes"
                )));
            };
            self.check_words(&strip_redirections(&strip_env_prefix(&words)))?;
        }
        Ok(())
    }

    fn check_words(&self, words: &[String]) -> Result<(), Refusal> {
        let Some(program) = words.first() else {
            return Ok(());
        };
        let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
        let program = program.trim_end_matches(".exe");
        match program {
            // arks is a git-compatible client; its push and tag verbs carry the same risks.
            "git" | "arks" => self.check_git(&git_subcommand(&words[1..])),
            "gh" => check_gh(&words[1..]),
            "cargo" if words.get(1).map(String::as_str) == Some("publish") => {
                Err(refusal("publishing a crate is owner-only"))
            }
            _ => Ok(()),
        }
    }

    fn check_git(&self, args: &[String]) -> Result<(), Refusal> {
        let Some(sub) = args.first().map(String::as_str) else {
            return Ok(());
        };
        let rest = &args[1..];
        match sub {
            "add"
                if rest
                    .iter()
                    .any(|a| matches!(a.as_str(), "." | "-A" | "--all" | ":/" | "*")) =>
            {
                Err(refusal(
                    "stage explicit paths only; `git add .`, `-A` and `--all` are not used",
                ))
            }
            "push" => self.check_push(rest),
            "tag" if !is_tag_listing(rest) => Err(refusal("creating or moving tags is owner-only")),
            "filter-branch" | "filter-repo" => Err(refusal("history rewriting is not allowed")),
            "rebase" if rest.iter().any(|a| a == "--root") => {
                Err(refusal("rewriting from the root commit is not allowed"))
            }
            _ => Ok(()),
        }
    }

    fn check_push(&self, rest: &[String]) -> Result<(), Refusal> {
        let mut lease = false;
        for a in rest {
            let a = a.as_str();
            if a == "--force-if-includes" {
                continue;
            }
            if let Some(v) = a.strip_prefix("--force-with-lease=") {
                // H4: only the explicit `<branch>:<expected-sha>` form.
                if v.split_once(':').is_some_and(|(b, sha)| {
                    !b.is_empty() && (sha.len() >= 7 || sha.starts_with('$'))
                }) {
                    lease = true;
                    continue;
                }
                return Err(refusal(
                    "use `--force-with-lease=<branch>:<expected-sha>` (commit-history H4)",
                ));
            }
            if a == "-f"
                || a.starts_with("--force")
                || a == "--mirror"
                || a == "--delete"
                || a == "-d"
                || a == "--tags"
                || a == "--follow-tags"
                || a == "--prune"
            {
                return Err(refusal(&format!(
                    "`git push {a}` is not allowed; push a topic branch normally"
                )));
            }
            if a.starts_with('+') {
                return Err(refusal("a `+refspec` is a force push and is not allowed"));
            }
        }
        // Positional arguments after the remote are refspecs.
        let positional: Vec<&str> = rest
            .iter()
            .map(String::as_str)
            .filter(|a| !a.starts_with('-'))
            .collect();
        for spec in positional.iter().skip(1) {
            if spec.starts_with(':') {
                return Err(refusal("deleting a remote ref is not allowed"));
            }
            if spec.starts_with("refs/tags/") || spec.contains(":refs/tags/") {
                return Err(refusal("pushing tags is owner-only"));
            }
            let dst = spec.rsplit(':').next().unwrap_or(spec);
            let dst = dst.trim_start_matches("refs/heads/");
            if dst == "HEAD" || dst == "@" {
                return Err(refusal(
                    "name the topic branch instead of HEAD, e.g. `git push -u origin <topic-branch>`",
                ));
            }
            if lease && !AGENT_PREFIXES.iter().any(|p| dst.starts_with(p)) {
                return Err(refusal(&format!(
                    "a lease push may only rewrite your own agent branch (`claude/…`, `codex/…`), not `{dst}`"
                )));
            }
            if self.protected_branches.iter().any(|b| b == dst) {
                return Err(refusal(&format!(
                    "`{dst}` is protected; push a topic branch and open a pull request"
                )));
            }
        }
        if positional.len() == 1 || positional.is_empty() {
            // `git push` / `git push origin` pushes the current branch; the hook
            // cannot see it, so require an explicit refspec.
            return Err(refusal(
                "name the branch explicitly, e.g. `git push -u origin <topic-branch>`",
            ));
        }
        Ok(())
    }
}

fn is_tag_listing(rest: &[String]) -> bool {
    rest.is_empty()
        || rest.iter().all(|a| {
            matches!(a.as_str(), "-l" | "--list" | "--contains" | "--points-at")
                || a.starts_with("--sort")
                || a.starts_with("--format")
        })
}

fn check_gh(args: &[String]) -> Result<(), Refusal> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["pr", "merge", rest @ ..]
            if !rest.contains(&"--auto") && !rest.contains(&"--disable-auto") =>
        {
            Err(refusal(
                "agents never merge directly; use `gh pr merge --auto --merge` (or the project's method)",
            ))
        }
        ["pr", "review", rest @ ..] if rest.iter().any(|x| *x == "--approve" || *x == "-a") => {
            Err(refusal("agents never approve pull requests"))
        }
        ["release", sub, ..] if matches!(*sub, "create" | "delete" | "edit" | "upload") => {
            Err(refusal("releases are owner-only"))
        }
        ["secret", sub, ..] if matches!(*sub, "set" | "delete" | "remove") => Err(refusal(
            "secret values are entered by the owner, never by an agent",
        )),
        ["ruleset", ..] if a.len() > 1 && !matches!(a[1], "list" | "view" | "check") => {
            Err(refusal("rulesets are applied by the owner from a file"))
        }
        ["api", rest @ ..] => check_gh_api(rest),
        _ => Ok(()),
    }
}

fn check_gh_api(rest: &[&str]) -> Result<(), Refusal> {
    let writes = rest
        .windows(2)
        .any(|w| matches!(w[0], "-X" | "--method") && !w[1].eq_ignore_ascii_case("GET"))
        || rest.iter().any(|x| {
            x.starts_with("--method=") && !x.eq_ignore_ascii_case("--method=GET")
                || x.starts_with("-X") && x.len() > 2 && !x[2..].eq_ignore_ascii_case("GET")
                || matches!(*x, "-f" | "-F" | "--field" | "--raw-field" | "--input")
        });
    if !writes {
        return Ok(());
    }
    let sensitive = rest.iter().any(|x| {
        x.contains("/rulesets")
            || x.contains("/branches/") && x.contains("/protection")
            || x.contains("/actions/secrets")
            || x.contains("/dependabot/secrets")
            || x.contains("/releases")
            || x.contains("/git/refs/tags")
            || x.contains("/merge")
            || x.contains("/collaborators")
            || x.contains("/hooks")
    });
    if sensitive {
        Err(refusal(
            "this GitHub API write is owner-only (rulesets, protection, secrets, releases, tags, merges, access, webhooks)",
        ))
    } else {
        Ok(())
    }
}

/// Refuses attribution trailers the common rules forbid.
fn check_trailers(line: &str) -> Option<Refusal> {
    for raw in line.split(['\n', '"', '\'']) {
        let l = raw.trim();
        let lower = l.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("co-authored-by:")
            && !rest
                .trim_end()
                .trim_end_matches('>')
                .ends_with("@users.noreply.github.com")
        {
            return Some(refusal(
                "a Co-authored-by trailer must use an @users.noreply.github.com address; name the tool in the PR body",
            ));
        }
        if lower.starts_with("claude-session:") {
            return Some(refusal("session links go in the PR body, not in commits"));
        }
    }
    None
}

/// Drops redirections (`2>&1`, `>/dev/null`, `> file`, `&>file`, `<in`).
fn strip_redirections(words: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_next = false;
    for w in words {
        if skip_next {
            skip_next = false;
            continue;
        }
        let body = w.trim_start_matches(|c: char| c.is_ascii_digit());
        let op = ["&>>", "&>", ">>", ">&", "<&", ">", "<"]
            .into_iter()
            .find(|op| body.starts_with(op));
        match op {
            Some(op) if body.len() == op.len() => skip_next = true,
            Some(_) => {}
            None => out.push(w.clone()),
        }
    }
    out
}

/// Removes heredoc bodies from `line`. Bodies fed to a shell (`sh`, `bash`,
/// `zsh`) are returned to be checked as commands; other bodies are data.
fn take_heredocs(line: &str) -> (String, Vec<String>) {
    let mut kept = Vec::new();
    let mut shell_bodies = Vec::new();
    let mut lines = line.lines();
    while let Some(l) = lines.next() {
        kept.push(l.to_string());
        let Some(pos) = l.find("<<") else { continue };
        let rest = &l[pos + 2..];
        if rest.starts_with('<') {
            continue; // here-string
        }
        let dash = rest.starts_with('-');
        let rest = rest.trim_start_matches('-').trim_start();
        let word: String = rest
            .trim_start_matches(['\'', '"', '\\'])
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if word.is_empty() {
            continue;
        }
        let mut body = Vec::new();
        for b in lines.by_ref() {
            let t = if dash { b.trim_start_matches('\t') } else { b };
            if t.trim_end() == word {
                break;
            }
            body.push(b.to_string());
        }
        let head = &l[..pos];
        let consumer = head
            .rsplit(['|', ';', '&'])
            .next()
            .unwrap_or(head)
            .split_whitespace()
            .find(|w| !w.contains('='))
            .map(|w| w.rsplit('/').next().unwrap_or(w).to_string())
            .unwrap_or_default();
        if matches!(consumer.as_str(), "sh" | "bash" | "zsh" | "dash") {
            shell_bodies.push(body.join("\n"));
        }
    }
    (kept.join("\n"), shell_bodies)
}

/// Drops leading `VAR=value` assignments and `env`.
fn strip_env_prefix(words: &[String]) -> Vec<String> {
    let mut i = 0;
    while i < words.len() {
        let w = &words[i];
        if w == "env"
            || w == "command"
            || w == "sudo"
            || (w.contains('=') && !w.starts_with('-') && !w.starts_with('='))
        {
            i += 1;
        } else {
            break;
        }
    }
    words[i..].to_vec()
}

/// Skips git global options (`-C dir`, `-c k=v`, `--git-dir=…`).
fn git_subcommand(args: &[String]) -> Vec<String> {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if matches!(a, "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace") {
            i += 2;
        } else if a.starts_with('-') {
            i += 1;
        } else {
            break;
        }
    }
    args[i.min(args.len())..].to_vec()
}

/// Splits on `;`, `&&`, `||`, `|`, `&` and newlines outside quotes.
fn split_segments(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                } else if c == '\\'
                    && q == '"'
                    && let Some(n) = chars.next()
                {
                    cur.push(n);
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    cur.push(c);
                }
                '&' if cur.ends_with('>') || cur.ends_with('<') || chars.peek() == Some(&'>') => {
                    cur.push(c);
                }
                ';' | '\n' | '|' | '&' => {
                    if (c == '|' || c == '&') && chars.peek() == Some(&c) {
                        chars.next();
                    }
                    out.push(std::mem::take(&mut cur));
                }
                '(' | ')' | '{' | '}' | '`' => out.push(std::mem::take(&mut cur)),
                _ => cur.push(c),
            },
        }
    }
    out.push(cur);
    out.into_iter()
        .map(|s| s.trim().trim_start_matches("$(").to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn refusal(msg: &str) -> Refusal {
    Refusal(format!("playpilot guard: {msg}"))
}
