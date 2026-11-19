use super::decide;
use crate::policy::Policy;

fn bash(cmd: &str) -> String {
    serde_json::json!({"tool_name": "Bash", "tool_input": {"command": cmd}}).to_string()
}

fn allowed(cmd: &str) -> bool {
    decide(&Policy::default(), &bash(cmd)).is_ok()
}

#[test]
fn allows_ordinary_work() {
    for cmd in [
        "cargo test --locked",
        "git status && git diff --stat",
        "git add crates/playpilot/src/policy.rs README.md",
        "git push -u origin claude/fix-guard",
        "git -C ../repo push origin claude/x",
        "gh pr create --base main --fill",
        "gh pr merge 12 --auto --merge",
        "gh api repos/o/r/pulls/12",
        "git push --force-with-lease=claude/x:0123456789abcdef origin claude/x",
        "git tag --list",
        "git commit -m 'fix: x' -m 'Co-authored-by: A <1+a@users.noreply.github.com>'",
    ] {
        assert!(allowed(cmd), "should allow: {cmd}");
    }
}

#[test]
fn refuses_boundary_crossings() {
    for cmd in [
        "git add .",
        "git add -A",
        "git push --force origin claude/x",
        "git push --force-with-lease origin claude/x",
        "git push --force-with-lease=main:0123456789abcdef origin main",
        "git push --force-with-lease=feature/x:0123456789abcdef origin feature/x",
        "git push origin +claude/x",
        "git push origin main",
        "git push origin HEAD:refs/heads/main",
        "git push origin HEAD",
        "git push",
        "git push origin :claude/old",
        "git push --tags origin",
        "git -C ../repo push -f origin claude/x",
        "cd x && git push origin master",
        "FOO=1 git push origin main",
        "git tag v1.0.0",
        "gh pr merge 12 --merge",
        "gh pr review 12 --approve",
        "gh release create v1",
        "gh secret set TOKEN",
        "gh api -X PUT repos/o/r/rulesets/1 --input r.json",
        "gh api repos/o/r/pulls/3/merge -X PUT",
        "cargo publish",
        "git filter-branch --all",
        "git commit -m 'x' -m 'Co-Authored-By: Bot <noreply@anthropic.com>'",
        "git commit -m 'x\n\nClaude-Session: https://example.invalid/s'",
    ] {
        assert!(!allowed(cmd), "should refuse: {cmd}");
    }
}

#[test]
fn protected_branches_are_configurable() {
    let p = Policy::from_env_value(Some("codex/rust, main"));
    assert!(decide(&p, &bash("git push origin codex/rust")).is_err());
    assert!(decide(&p, &bash("git push origin claude/topic")).is_ok());
}

#[test]
fn ignores_other_tools_and_rejects_bad_input() {
    let p = Policy::default();
    let edit =
        serde_json::json!({"tool_name": "Edit", "tool_input": {"file_path": "x"}}).to_string();
    assert!(decide(&p, &edit).is_ok());
    assert!(decide(&p, "not json").is_err());
    let ps = serde_json::json!({"tool_name": "PowerShell", "tool_input": {"command": "git push -f origin x"}}).to_string();
    assert!(decide(&p, &ps).is_err());
}

mod history_rules {
    use crate::history::{Commit, Method, check, conventional, parse_log};

    fn commit(subject: &str) -> Commit {
        Commit {
            sha: "0123456789abcdef".into(),
            parents: 1,
            author: "1+o@users.noreply.github.com".into(),
            committer: "1+o@users.noreply.github.com".into(),
            subject: subject.into(),
            body: String::new(),
        }
    }

    #[test]
    fn conventional_subjects() {
        assert!(conventional("feat(guard): allow lease pushes").is_ok());
        assert!(conventional("fix!: drop old flag").is_ok());
        assert!(conventional("Update stuff").is_err());
        assert!(conventional("feature: x").is_err());
        assert!(conventional(&format!("docs: {}", "x".repeat(80))).is_err());
    }

    #[test]
    fn squash_needs_a_valid_title_but_not_valid_commits() {
        let commits = [commit("wip on things")];
        let p = check(&commits, Method::Squash, Some("feat: add history check"));
        assert_eq!(p.len(), 1, "{p:?}"); // only the WIP marker
        assert!(!check(&[], Method::Squash, Some("bad title")).is_empty());
    }

    #[test]
    fn merge_method_checks_every_commit_and_merges() {
        let mut m = commit("Merge branch 'main' into claude/x");
        m.parents = 2;
        let mut web = commit("fix: x");
        web.committer = "noreply@github.com".into();
        let mut trailer = commit("docs: y");
        trailer.body = "Co-Authored-By: Bot <noreply@anthropic.com>".into();
        let p = check(&[m, web, trailer], Method::Merge, None);
        assert_eq!(p.len(), 3, "{p:?}"); // merge commit, its subject, the trailer
    }

    #[test]
    fn parses_git_log_records() {
        let text = "a1\u{1f}p1\u{1f}x@users.noreply.github.com\u{1f}noreply@github.com\u{1f}feat: a\u{1f}body\n\u{1e}\nb2\u{1f}p1 p2\u{1f}e\u{1f}e\u{1f}Merge\u{1f}\u{1e}\n";
        let c = parse_log(text);
        assert_eq!(c.len(), 2);
        assert_eq!(c[1].parents, 2);
    }
}

mod surface_rules {
    use crate::surface::{Item, scan};

    fn item(text: &str) -> Item {
        Item {
            location: "x".into(),
            text: text.into(),
        }
    }

    #[test]
    fn deny_terms_report_index_not_term() {
        let f = scan(
            &[item("deploy to Secret-Host now")],
            &["secret-host".into()],
        );
        assert_eq!(f, vec!["x: matches deny term #1".to_string()]);
    }

    #[test]
    fn emails_and_paths() {
        assert!(scan(&[item("author 1+o@users.noreply.github.com")], &[]).is_empty());
        assert!(scan(&[item("mail security@playpilot.io or a@example.com")], &[]).is_empty());
        assert_eq!(
            scan(&[item(&format!("ping someone@{}", "mail.test"))], &[]).len(),
            1
        );
        assert_eq!(
            scan(&[item(&format!("see /home/{}/project", "alice"))], &[]).len(),
            1
        );
        assert!(
            scan(
                &[item("clone into /home/user/repo or C:\\Users\\<you>")],
                &[]
            )
            .is_empty()
        );
        assert_eq!(
            scan(&[item(&format!("C:\\Users\\{}\\x", "bob"))], &[]).len(),
            1
        );
    }

    #[test]
    fn empty_deny_list_is_allowed() {
        assert!(scan(&[item("anything")], &["".into(), "  ".into()]).is_empty());
    }
}
