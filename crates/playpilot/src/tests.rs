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
fn reported_false_refusals_are_allowed() {
    for cmd in [
        "git push -u origin claude/x 2>&1 | tail -1",
        "git push origin claude/x >/dev/null 2>&1",
        "git push origin claude/x &> push.log",
        "git push --force-with-lease=claude/x:$old origin claude/x",
        "python3 - <<'EOF'\nprint('''a \"quoted\" text''')\nEOF",
        "cat <<EOF > notes.md\nthis mentions git push origin main as text\nEOF",
    ] {
        assert!(allowed(cmd), "should allow: {cmd}");
    }
}

#[test]
fn arks_is_checked_like_git() {
    assert!(!allowed("arks push origin main"));
    assert!(!allowed("arks push --force origin claude/x"));
    assert!(!allowed("arks tag v1.0.0"));
    assert!(allowed("arks push -u origin claude/x"));
    assert!(allowed("arks status"));
}

#[test]
fn heredocs_fed_to_a_shell_are_checked() {
    assert!(!allowed("bash <<'EOF'\ngit push origin main\nEOF"));
    assert!(!allowed(
        "sh -s <<EOF\ngit push --force origin claude/x\nEOF"
    ));
    assert!(!allowed(
        "git push --force-with-lease=main:$old origin main"
    ));
    assert!(!allowed("git push origin main 2>&1"));
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
    fn github_service_senders_are_not_personal() {
        let msg = "Signed-off-by: dependabot[bot] <support@github.com>";
        assert!(scan(&[item(msg)], &[]).is_empty());
        assert_eq!(
            scan(&[item(&format!("{}@github.com", "octo"))], &[]).len(),
            1
        );
    }

    #[test]
    fn version_pins_are_not_emails() {
        assert!(scan(&[item("remote_theme: owner/theme@v1.7.1")], &[]).is_empty());
        assert!(scan(&[item("uses: actions/checkout@3d3c42e5")], &[]).is_empty());
    }

    #[test]
    fn empty_deny_list_is_allowed() {
        assert!(scan(&[item("anything")], &["".into(), "  ".into()]).is_empty());
    }
}

mod validate_rules {
    use crate::validate::{check, parse};

    fn res(file: &str, body: &str) -> crate::validate::Resource {
        parse(file, &format!("apiVersion = \"playpilot.io/v1\"\n{body}")).expect(file)
    }

    #[test]
    fn rejects_wrong_api_version_and_kind() {
        assert!(
            parse(
                "a",
                "apiVersion = \"v0\"\nkind = \"Lane\"\n[metadata]\nname = \"x\"\n[spec]\n"
            )
            .is_err()
        );
        assert!(parse("a", "apiVersion = \"playpilot.io/v1\"\nkind = \"Nope\"\n[metadata]\nname = \"x\"\n[spec]\n").is_err());
    }

    #[test]
    fn references_and_coverage() {
        let lane = res(
            "l",
            "kind = \"Lane\"\n[metadata]\nname = \"win\"\n[spec]\nrequires = [\"os:windows\"]\n",
        );
        let project = res(
            "p",
            "kind = \"Project\"\n[metadata]\nname = \"p\"\n[spec]\nlanes = [\"win\", \"ghost\"]\n",
        );
        let session = res(
            "s",
            "kind = \"Session\"\n[metadata]\nname = \"s\"\n[spec]\nproject = \"p\"\nrole = \"nobody\"\n",
        );
        let p = check(&[lane.clone(), project.clone(), session]);
        assert!(p.iter().any(|x| x.contains("\"ghost\"")), "{p:?}");
        assert!(p.iter().any(|x| x.contains("\"nobody\"")), "{p:?}");
        assert!(
            p.iter()
                .any(|x| x.contains("no device can serve lane \"win\"")),
            "{p:?}"
        );
        let device = res(
            "d",
            "kind = \"Device\"\n[metadata]\nname = \"d\"\n[spec]\ncapabilities = [\"os:windows\"]\n",
        );
        assert!(
            !check(&[lane, project, device])
                .iter()
                .any(|x| x.contains("no device"))
        );
    }

    #[test]
    fn literal_secrets_and_duplicates() {
        let w = res(
            "w",
            "kind = \"Webhook\"\n[metadata]\nname = \"w\"\n[spec]\ntoken = \"abc\"\nsigning = { secretRef = \"S\" }\n",
        );
        let w2 = res(
            "w2",
            "kind = \"Webhook\"\n[metadata]\nname = \"w\"\n[spec]\n",
        );
        let p = check(&[w, w2]);
        assert_eq!(p.len(), 2, "{p:?}");
    }
}

mod mail_rules {
    use std::cell::RefCell;

    use crate::mail::github::stored_from_comment;
    use crate::mail::message::{Envelope, Stored, valid_actor};
    use crate::mail::transport::{Receipt, Transport};
    use crate::mail::{days_ago, inbox, send};

    fn env(from: &str, to: &str) -> Envelope {
        Envelope {
            from: from.into(),
            to: to.into(),
            subject: "s".into(),
            re: Some("org/repo#1".into()),
            body: "line one\nline two".into(),
        }
    }

    #[test]
    fn envelope_round_trips_and_rejects_other_comments() {
        let e = env("kinekino", "design");
        assert_eq!(Envelope::parse(&e.render()), Some(e));
        assert_eq!(Envelope::parse("just a comment"), None);
        assert_eq!(
            Envelope::parse("pmcp-mail v1\nfrom: Bad Name\nto: design\n---\nx"),
            None
        );
        assert!(valid_actor("oss-desktop-2") && !valid_actor("-x") && !valid_actor("A"));
    }

    #[test]
    fn github_comment_maps_receipts() {
        let c = serde_json::json!({
            "id": 7, "created_at": "2026-11-19T20:09:04Z",
            "body": env("oss-desktop", "design").render(),
            "reactions": {"eyes": 1, "+1": 0}
        });
        let m = stored_from_comment(&c).unwrap();
        assert!(m.read && !m.acked && m.id == 7);
    }

    #[derive(Default)]
    struct Fake {
        boxes: RefCell<Vec<(String, Stored)>>,
    }

    impl Transport for Fake {
        fn send(&self, e: &Envelope) -> Result<u64, String> {
            let mut b = self.boxes.borrow_mut();
            let id = b.len() as u64 + 1;
            let stored = Stored {
                id,
                created_at: format!("2026-11-{:02}T00:00:00Z", 10 + id),
                envelope: e.clone(),
                read: false,
                acked: false,
            };
            b.push((e.to.clone(), stored));
            Ok(id)
        }
        fn list(&self, actor: &str) -> Result<Vec<Stored>, String> {
            Ok(self
                .boxes
                .borrow()
                .iter()
                .filter(|(b, _)| b == actor)
                .map(|(_, m)| m.clone())
                .collect())
        }
        fn receipt(&self, id: u64, kind: Receipt) -> Result<(), String> {
            for (_, m) in self
                .boxes
                .borrow_mut()
                .iter_mut()
                .filter(|(_, m)| m.id == id)
            {
                match kind {
                    Receipt::Read => m.read = true,
                    Receipt::Ack => m.acked = true,
                }
            }
            Ok(())
        }
    }

    #[test]
    fn inbox_hides_acked_and_old_broadcasts() {
        let t = Fake::default();
        send(&t, env("kinekino", "design")).unwrap(); // id 1, 11th
        send(&t, env("design", "all")).unwrap(); // id 2, 12th: own broadcast
        send(&t, env("arks", "all")).unwrap(); // id 3, 13th
        send(&t, env("svgif", "design")).unwrap(); // id 4, 14th
        t.receipt(4, Receipt::Ack).unwrap();
        let ids = |v: Vec<Stored>| v.into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(
            ids(inbox(&t, "design", false, "2026-11-01T00:00:00Z").unwrap()),
            vec![1, 3]
        );
        assert_eq!(
            ids(inbox(&t, "design", false, "2026-11-20T00:00:00Z").unwrap()),
            vec![1]
        );
        assert_eq!(
            ids(inbox(&t, "design", true, "2026-11-20T00:00:00Z").unwrap()),
            vec![1, 3, 4]
        );
    }

    #[test]
    fn send_validates_input() {
        let t = Fake::default();
        assert!(
            send(
                &t,
                Envelope {
                    body: " ".into(),
                    ..env("a", "b")
                }
            )
            .is_err()
        );
        assert!(
            send(
                &t,
                Envelope {
                    subject: "".into(),
                    ..env("a", "b")
                }
            )
            .is_err()
        );
        assert!(send(&t, env("A", "b")).is_err());
    }

    #[test]
    fn days_ago_formats_utc() {
        // 2026-11-19T20:00:00Z = 1795118400
        assert_eq!(days_ago(1_795_118_400, 7), "2026-11-12T20:00:00Z");
        assert_eq!(days_ago(0, 0), "1970-01-01T00:00:00Z");
    }
}

#[test]
fn refuses_bare_tag_names_in_push() {
    for cmd in [
        "git push origin v1.0.0",
        "git push origin 1.2",
        "git push origin v2.0.0-rc.1",
        "git push origin v1.0.0:v1.0.0",
        "arks push origin v0.3.0",
    ] {
        assert!(!allowed(cmd), "should refuse: {cmd}");
    }
    assert!(allowed("git push -u origin claude/v1.0-notes"));
    assert!(allowed("git push -u origin claude/release-v1.0.0"));
}

#[test]
fn refuses_pushing_a_known_local_tag() {
    let p = Policy {
        local_tags: vec!["stable".into()],
        ..Policy::default()
    };
    assert!(decide(&p, &bash("git push origin stable")).is_err());
    assert!(decide(&p, &bash("git push origin claude/stable-docs")).is_ok());
}

mod render_rules {
    use crate::render::{GuardSpec, guard_command, guard_spec, lock_rev, merge_settings};
    use crate::validate::parse;

    const REV: &str = "0123456789abcdef0123456789abcdef01234567";

    fn spec() -> GuardSpec {
        GuardSpec {
            rev: REV.into(),
            protected: vec!["main".into(), "codex/rust".into()],
        }
    }

    #[test]
    fn reads_the_lock_rev() {
        assert_eq!(
            lock_rev(&format!("version = \"unreleased\"\nrev = \"{REV}\"\n")).unwrap(),
            REV
        );
        assert!(lock_rev("rev = \"abc\"").is_err());
        assert!(lock_rev("version = \"x\"").is_err());
    }

    #[test]
    fn builds_the_spec_from_the_project() {
        let p = parse(
            "p.toml",
            "apiVersion = \"playpilot.io/v1\"\nkind = \"Project\"\n[metadata]\nname = \"k\"\n[spec]\nprotected = [\"main\"]\n",
        )
        .unwrap();
        let s = guard_spec(std::slice::from_ref(&p), "k", REV.into()).unwrap();
        assert_eq!(s.protected, vec!["main".to_string()]);
        assert!(guard_spec(std::slice::from_ref(&p), "other", REV.into()).is_err());
        let bad = parse(
            "b.toml",
            "apiVersion = \"playpilot.io/v1\"\nkind = \"Project\"\n[metadata]\nname = \"b\"\n[spec]\nprotected = [\"main; rm -rf /\"]\n",
        )
        .unwrap();
        assert!(guard_spec(&[bad], "b", REV.into()).is_err());
    }

    #[test]
    fn command_pins_the_rev_and_branches() {
        let c = guard_command(&spec());
        assert!(c.contains(&format!("R={REV};")));
        assert!(c.contains("PLAYPILOT_PROTECTED_BRANCHES=main,codex/rust exec"));
        assert!(c.contains("exit 2"));
        let words = shlex::split(&c).unwrap();
        assert_eq!(words[..2], ["sh".to_string(), "-c".to_string()]);
    }

    #[test]
    fn writes_fresh_settings() {
        let out = merge_settings(None, &spec()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let hook = &v["hooks"]["PreToolUse"][0];
        assert_eq!(hook["matcher"], "Bash|PowerShell");
        assert_eq!(hook["hooks"][0]["timeout"], 240);
        assert!(out.ends_with("}\n"));
    }

    #[test]
    fn replaces_an_old_guard_and_keeps_everything_else() {
        let old = r#"{
  "permissions": {"allow": ["Bash(ls)"]},
  "hooks": {
    "PreToolUse": [
      {"matcher": "Edit", "hooks": [{"type": "command", "command": "lint-edit"}]},
      {"matcher": "Bash|PowerShell", "hooks": [{"type": "command", "command": "sh -c 'exec playpilot guard'", "timeout": 240}]}
    ],
    "Stop": [{"hooks": [{"type": "command", "command": "notify"}]}]
  },
  "env": {"PLAYPILOT_PROTECTED_BRANCHES": "main", "OTHER": "1"}
}"#;
        let out = merge_settings(Some(old), &spec()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["permissions", "hooks", "env"]);
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["hooks"][0]["command"], "lint-edit");
        assert!(
            pre[1]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .contains(REV)
        );
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "notify");
        assert_eq!(v["env"], serde_json::json!({"OTHER": "1"}));
        // Rendering twice changes nothing.
        assert_eq!(merge_settings(Some(&out), &spec()).unwrap(), out);
    }

    #[test]
    fn refuses_settings_that_are_not_an_object() {
        assert!(merge_settings(Some("[]"), &spec()).is_err());
        assert!(merge_settings(Some("{"), &spec()).is_err());
    }
}
