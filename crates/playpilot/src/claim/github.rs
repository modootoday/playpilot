//! Adapter: claims on GitHub issues and pull requests through `gh api`.

use serde_json::json;

use super::tracker::{Comment, Tracker, parse_item};

pub struct GitHubTracker;

impl Tracker for GitHubTracker {
    fn comments(&self, item: &str) -> Result<Vec<Comment>, String> {
        let (repo, n) = parse_item(item)?;
        let mut out = Vec::new();
        for page in 1.. {
            let path = format!("repos/{repo}/issues/{n}/comments?per_page=100&page={page}");
            let batch = crate::gh::api(&[&path], None)?;
            let items = batch.as_array().cloned().unwrap_or_default();
            out.extend(items.iter().filter_map(|c| {
                Some(Comment {
                    body: c.get("body")?.as_str()?.to_string(),
                    created_at: c.get("created_at")?.as_str()?.to_string(),
                })
            }));
            if items.len() < 100 {
                break;
            }
        }
        Ok(out)
    }

    fn comment(&self, item: &str, body: &str) -> Result<(), String> {
        let (repo, n) = parse_item(item)?;
        let path = format!("repos/{repo}/issues/{n}/comments");
        let input = json!({ "body": body }).to_string();
        crate::gh::api(&["-X", "POST", &path, "--input", "-"], Some(&input)).map(|_| ())
    }

    fn add_label(&self, item: &str, label: &str) -> Result<(), String> {
        let (repo, n) = parse_item(item)?;
        let path = format!("repos/{repo}/issues/{n}/labels");
        let input = json!({ "labels": [label] }).to_string();
        crate::gh::api(&["-X", "POST", &path, "--input", "-"], Some(&input)).map(|_| ())
    }

    fn remove_label(&self, item: &str, label: &str) -> Result<(), String> {
        let (repo, n) = parse_item(item)?;
        let path = format!("repos/{repo}/issues/{n}/labels/{label}");
        match crate::gh::api(&["-X", "DELETE", &path], None) {
            Ok(_) => Ok(()),
            Err(e) if e.contains("Label does not exist") || e.contains("404") => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn labelled(&self, repo: &str, label: &str) -> Result<Vec<String>, String> {
        parse_item(&format!("{repo}#1"))?;
        let mut out = Vec::new();
        for page in 1.. {
            let path =
                format!("repos/{repo}/issues?labels={label}&state=open&per_page=100&page={page}");
            let batch = crate::gh::api(&[&path], None)?;
            let items = batch.as_array().cloned().unwrap_or_default();
            out.extend(
                items
                    .iter()
                    .filter_map(|i| Some(format!("{repo}#{}", i.get("number")?.as_u64()?))),
            );
            if items.len() < 100 {
                break;
            }
        }
        Ok(out)
    }
}
