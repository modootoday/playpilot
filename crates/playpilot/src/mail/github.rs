//! Infrastructure: mailboxes as never-merged draft pull requests, through the `gh` CLI.
//!
//! The mailbox of actor `x` is the open pull request from branch `mailbox/x`;
//! a message is a comment on it; receipts are the 👀 (read) and 👍 (ack) reactions.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

use super::message::{Envelope, Stored};
use super::transport::{Receipt, Transport};

pub struct GitHub {
    /// `owner/name` of the repository that holds the mailboxes.
    pub repo: String,
}

impl GitHub {
    fn owner(&self) -> &str {
        self.repo.split('/').next().unwrap_or_default()
    }

    fn api(&self, args: &[&str], stdin: Option<&str>) -> Result<Value, String> {
        let mut cmd = Command::new("gh");
        cmd.arg("api").args(args);
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("could not run gh: {e}"))?;
        if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(input.as_bytes())
                .map_err(|e| format!("could not write to gh: {e}"))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("gh failed: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        serde_json::from_slice(&out.stdout).map_err(|e| format!("gh returned non-JSON: {e}"))
    }

    /// The pull request number of `actor`'s mailbox.
    pub fn mailbox(&self, actor: &str) -> Result<u64, String> {
        let path = format!(
            "repos/{}/pulls?state=open&per_page=1&head={}:mailbox/{actor}",
            self.repo,
            self.owner()
        );
        self.api(&[&path], None)?
            .as_array()
            .and_then(|a| a.first())
            .and_then(|pr| pr.get("number"))
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("no open mailbox for `{actor}` in {}", self.repo))
    }
}

/// Turns one issue comment into a stored message, if it is one.
pub fn stored_from_comment(c: &Value) -> Option<Stored> {
    let envelope = Envelope::parse(c.get("body")?.as_str()?)?;
    let count = |k: &str| {
        c.pointer(&format!("/reactions/{k}"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    Some(Stored {
        id: c.get("id")?.as_u64()?,
        created_at: c.get("created_at")?.as_str()?.to_string(),
        envelope,
        read: count("eyes") > 0,
        acked: count("+1") > 0,
    })
}

impl Transport for GitHub {
    fn send(&self, mailbox: &str, envelope: &Envelope) -> Result<u64, String> {
        let number = self.mailbox(mailbox)?;
        let path = format!("repos/{}/issues/{number}/comments", self.repo);
        let body = serde_json::json!({ "body": envelope.render() }).to_string();
        let created = self.api(&["-X", "POST", &path, "--input", "-"], Some(&body))?;
        created
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| "GitHub did not return a comment id".to_string())
    }

    fn actors(&self) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        for page in 1.. {
            let path = format!(
                "repos/{}/pulls?state=open&per_page=100&page={page}",
                self.repo
            );
            let batch = self.api(&[&path], None)?;
            let items = batch.as_array().cloned().unwrap_or_default();
            out.extend(items.iter().filter_map(|pr| {
                let same_repo = pr.pointer("/head/repo/full_name").and_then(Value::as_str)
                    == Some(self.repo.as_str());
                let actor = pr
                    .pointer("/head/ref")?
                    .as_str()?
                    .strip_prefix("mailbox/")?;
                same_repo.then(|| actor.to_string())
            }));
            if items.len() < 100 {
                break;
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    fn list(&self, actor: &str) -> Result<Vec<Stored>, String> {
        let number = self.mailbox(actor)?;
        let mut out = Vec::new();
        for page in 1.. {
            let path = format!(
                "repos/{}/issues/{number}/comments?per_page=100&page={page}",
                self.repo
            );
            let batch = self.api(&[&path], None)?;
            let items = batch.as_array().cloned().unwrap_or_default();
            out.extend(items.iter().filter_map(stored_from_comment));
            if items.len() < 100 {
                break;
            }
        }
        Ok(out)
    }

    fn receipt(&self, id: u64, kind: Receipt) -> Result<(), String> {
        let content = match kind {
            Receipt::Read => "eyes",
            Receipt::Ack => "+1",
        };
        let path = format!("repos/{}/issues/comments/{id}/reactions", self.repo);
        self.api(
            &["-X", "POST", &path, "-f", &format!("content={content}")],
            None,
        )
        .map(|_| ())
    }
}
