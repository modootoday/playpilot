//! The compiled fleet configuration the Worker serves. Git authors it; the
//! deploy step hands it to the Worker as a KV value (namespace `CONFIG`, key
//! `config.json`) or, without that namespace, as a var. The Worker only adds its
//! own narrow-only `ops` and an ETag.

use serde_json::Value;

use super::ids::valid_rev;
use super::ops::{Ops, OpsTable};

pub const API_VERSION: &str = "playpilot.io/v1";

#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    pub body: Value,
    pub rev: String,
}

impl Compiled {
    pub fn parse(text: &str) -> Result<Self, String> {
        let body: Value = serde_json::from_str(text).map_err(|e| format!("config: {e}"))?;
        if body.get("apiVersion").and_then(Value::as_str) != Some(API_VERSION) {
            return Err(format!("config: apiVersion must be {API_VERSION}"));
        }
        let rev = body.get("rev").and_then(Value::as_str).unwrap_or("");
        if !valid_rev(rev) {
            return Err("config: rev must be a full commit SHA".into());
        }
        if !body.get("resources").is_some_and(Value::is_array) {
            return Err("config: resources must be an array".into());
        }
        Ok(Self {
            rev: rev.to_string(),
            body,
        })
    }

    fn resources(&self) -> impl Iterator<Item = (&str, &str, &Value)> {
        self.body["resources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let kind = r.get("kind")?.as_str()?;
                let name = r.get("metadata")?.get("name")?.as_str()?;
                Some((kind, name, r))
            })
    }

    /// `spec.ops` of every resource, keyed `Kind/name`: the file side.
    pub fn file_ops(&self) -> OpsTable {
        let mut t = OpsTable::default();
        for (kind, name, r) in self.resources() {
            if let Some(ops) = r.get("spec").and_then(|s| s.get("ops"))
                && let Ok(ops) = Ops::from_value(ops)
            {
                t.0.insert(format!("{kind}/{name}"), ops);
            }
        }
        t
    }

    /// Names of `Lane` resources.
    pub fn lanes(&self) -> Vec<String> {
        self.resources()
            .filter(|(kind, _, _)| *kind == "Lane")
            .map(|(_, name, _)| name.to_string())
            .collect()
    }

    /// The response body: the compiled config with the Worker's `ops`.
    pub fn serve(&self, worker_ops: &OpsTable) -> Value {
        let mut body = self.body.clone();
        if !worker_ops.0.is_empty() {
            body["ops"] = worker_ops.to_value();
        }
        body
    }

    /// Strong ETag over the rev and the Worker ops, so an ops change refetches.
    pub fn etag(&self, worker_ops: &OpsTable) -> String {
        let ops = worker_ops.to_value().to_string();
        format!("\"{}-{:016x}\"", self.rev, fnv1a(ops.as_bytes()))
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Where the compiled config text is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The KV namespace is bound: its value is authoritative, even when absent.
    Kv(Option<String>),
    /// No KV namespace: the var or secret.
    Var(Option<String>),
}

/// The config text, or why there is none. A bound namespace without the key
/// is an error rather than a silent fall back to an older var.
pub fn config_text(source: Source, kv: &str, key: &str, var: &str) -> Result<String, String> {
    let nonempty = |t: Option<String>| t.filter(|t| !t.trim().is_empty());
    match source {
        Source::Kv(t) => nonempty(t).ok_or(format!("{kv} has no `{key}`")),
        Source::Var(t) => nonempty(t).ok_or(format!("{var} is not set")),
    }
}

/// `If-None-Match` handling: `*` or any listed tag (weak or strong) matches.
pub fn not_modified(if_none_match: Option<&str>, etag: &str) -> bool {
    let Some(header) = if_none_match else {
        return false;
    };
    header
        .split(',')
        .map(str::trim)
        .any(|t| t == "*" || t.strip_prefix("W/").unwrap_or(t) == etag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> String {
        format!(
            r#"{{"apiVersion":"playpilot.io/v1","rev":"{}","resources":[
              {{"kind":"Lane","metadata":{{"name":"build"}},"spec":{{"ops":{{"capacity":2}}}}}},
              {{"kind":"Device","metadata":{{"name":"node-a"}},"spec":{{"ops":{{"paused":true}}}}}},
              {{"kind":"Lane","metadata":{{"name":"docs"}},"spec":{{}}}}
            ]}}"#,
            "c".repeat(40)
        )
    }

    #[test]
    fn parses_and_reads_file_ops() {
        let c = Compiled::parse(&text()).unwrap();
        assert_eq!(c.lanes(), vec!["build", "docs"]);
        let ops = c.file_ops();
        assert_eq!(ops.get("Lane", "build").capacity, Some(2));
        assert!(ops.get("Device", "node-a").is_paused());
        assert_eq!(ops.get("Lane", "docs"), Ops::default());
    }

    #[test]
    fn refuses_bad_configs() {
        assert!(Compiled::parse(r#"{"apiVersion":"v0","rev":"x","resources":[]}"#).is_err());
        let no_res = format!(
            r#"{{"apiVersion":"playpilot.io/v1","rev":"{}"}}"#,
            "c".repeat(40)
        );
        assert!(Compiled::parse(&no_res).is_err());
        assert!(Compiled::parse("nope").is_err());
    }

    #[test]
    fn etag_follows_worker_ops() {
        let c = Compiled::parse(&text()).unwrap();
        let none = OpsTable::default();
        let some = OpsTable::parse(r#"{"Lane/build":{"paused":true}}"#).unwrap();
        assert_ne!(c.etag(&none), c.etag(&some));
        assert!(c.etag(&none).contains(&c.rev));
        assert!(c.serve(&none).get("ops").is_none());
        assert_eq!(c.serve(&some)["ops"]["Lane/build"]["paused"], true);
    }

    #[test]
    fn kv_binding_wins_over_the_var() {
        let pick = |s| config_text(s, "CONFIG", "config.json", "VAR");
        assert_eq!(pick(Source::Kv(Some("{}".into()))).unwrap(), "{}");
        assert_eq!(pick(Source::Var(Some("[]".into()))).unwrap(), "[]");
        assert_eq!(
            pick(Source::Kv(None)).unwrap_err(),
            "CONFIG has no `config.json`"
        );
        assert_eq!(
            pick(Source::Kv(Some(" ".into()))).unwrap_err(),
            "CONFIG has no `config.json`"
        );
        assert_eq!(pick(Source::Var(None)).unwrap_err(), "VAR is not set");
    }

    #[test]
    fn conditional_requests() {
        let tag = "\"abc\"";
        assert!(not_modified(Some("\"abc\""), tag));
        assert!(not_modified(Some("W/\"abc\", \"x\""), tag));
        assert!(not_modified(Some("*"), tag));
        assert!(!not_modified(Some("\"x\""), tag));
        assert!(!not_modified(None, tag));
    }
}
