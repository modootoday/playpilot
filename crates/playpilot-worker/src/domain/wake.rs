//! Waking a session when mail arrives. A cloud session can only be woken by
//! firing its scheduled task (routine), which starts a new session with the
//! fire text as untrusted input. The text therefore never carries message
//! content: it only says how many messages wait and which tool reads them.
//! Fires are rate limited upstream, so the bus debounces them per actor.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Value, json};

use super::ids::valid_name;

/// At most one fire per actor in this window while its mail stays unacked.
pub const DEBOUNCE_SECS: u64 = 600;
pub const API_BASE: &str = "https://api.anthropic.com";
pub const API_VERSION: &str = "2026-10-21";
/// Upper bound the routine API accepts for the fire text.
pub const MAX_TEXT: usize = 65_536;

/// The routine to fire for one actor and its per-task token.
#[derive(Clone, PartialEq, Eq)]
pub struct Target {
    pub routine: String,
    pub token: String,
}

impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Target")
            .field("routine", &self.routine)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WakeMap(BTreeMap<String, Target>);

fn valid_routine(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
}

impl WakeMap {
    /// `{"<actor>": {"routine": "<routine id>", "token": "<per-task token>"}}`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|_| "wake: not valid JSON")?;
        let obj = v.as_object().ok_or("wake must be a JSON object")?;
        let mut out = BTreeMap::new();
        for (actor, entry) in obj {
            if !valid_name(actor) || actor == "all" {
                return Err(format!("wake: `{actor}` is not a valid actor name"));
            }
            let get = |k: &str| entry.get(k).and_then(Value::as_str).map(str::trim);
            let routine = get("routine").filter(|r| valid_routine(r));
            let token = get("token").filter(|t| !t.is_empty() && !t.contains(['\r', '\n']));
            let (Some(routine), Some(token)) = (routine, token) else {
                return Err(format!(
                    "wake: `{actor}` needs a routine id and a token string"
                ));
            };
            out.insert(
                actor.clone(),
                Target {
                    routine: routine.into(),
                    token: token.into(),
                },
            );
        }
        Ok(Self(out))
    }

    pub fn get(&self, actor: &str) -> Option<&Target> {
        self.0.get(actor)
    }

    pub fn actors(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }
}

/// Whether to fire for an actor that has `unacked` messages now, given the
/// time of its last fire. Acking every message clears the last fire, so the
/// next message wakes at once.
pub fn should_wake(last_fire: Option<u64>, now: u64, unacked: usize) -> bool {
    unacked > 0 && last_fire.is_none_or(|t| now.saturating_sub(t) >= DEBOUNCE_SECS)
}

pub fn fire_text(actor: &str, unacked: usize) -> String {
    format!(
        "playpilot mail: {unacked} unacked message(s) for {actor}; read them with the playpilot mail tools (mail_inbox) and handle them."
    )
}

/// One HTTP request, independent of the runtime's fetch API.
#[derive(Clone, PartialEq, Eq)]
pub struct FireRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: String,
}

impl fmt::Debug for FireRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FireRequest")
            .field("url", &self.url)
            .field("body", &self.body)
            .finish_non_exhaustive()
    }
}

pub fn fire_request(target: &Target, actor: &str, unacked: usize) -> FireRequest {
    let mut text = fire_text(actor, unacked);
    text.truncate(MAX_TEXT);
    FireRequest {
        url: format!("{API_BASE}/v1/claude_code/routines/{}/fire", target.routine),
        headers: vec![
            ("Authorization", format!("Bearer {}", target.token)),
            ("anthropic-version", API_VERSION.into()),
            ("Content-Type", "application/json".into()),
        ],
        body: json!({ "text": text }).to_string(),
    }
}

/// One actor to wake after a send, with its unacked count.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Due {
    pub actor: String,
    pub unacked: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounces_per_actor() {
        assert!(!should_wake(None, 1_000, 0));
        assert!(should_wake(None, 1_000, 1));
        assert!(!should_wake(Some(1_000), 1_000, 3));
        assert!(!should_wake(Some(1_000), 1_000 + DEBOUNCE_SECS - 1, 3));
        assert!(should_wake(Some(1_000), 1_000 + DEBOUNCE_SECS, 3));
        assert!(!should_wake(Some(1_000), 1_000 + DEBOUNCE_SECS, 0));
        assert!(!should_wake(Some(2_000), 1_000, 1));
    }

    #[test]
    fn builds_the_fire_request() {
        let t = Target {
            routine: "trig_01example".into(),
            token: "secret-value".into(),
        };
        let r = fire_request(&t, "node-a", 2);
        assert_eq!(
            r.url,
            "https://api.anthropic.com/v1/claude_code/routines/trig_01example/fire"
        );
        assert!(
            r.headers
                .contains(&("Authorization", "Bearer secret-value".into()))
        );
        assert!(
            r.headers
                .contains(&("anthropic-version", "2026-10-21".into()))
        );
        assert!(
            r.headers
                .contains(&("Content-Type", "application/json".into()))
        );
        let body: Value = serde_json::from_str(&r.body).unwrap();
        assert_eq!(
            body,
            json!({"text": "playpilot mail: 2 unacked message(s) for node-a; read them with the playpilot mail tools (mail_inbox) and handle them."})
        );
        assert!(!format!("{r:?}").contains("secret-value"));
        assert!(!format!("{t:?}").contains("secret-value"));
    }

    #[test]
    fn parses_wake_map() {
        let m = WakeMap::parse(r#"{"node-a":{"routine":"trig_01x","token":"tok"}}"#).unwrap();
        assert_eq!(m.get("node-a").unwrap().routine, "trig_01x");
        assert!(m.get("node-b").is_none());
        assert_eq!(m.actors(), ["node-a"]);
        for bad in [
            "[]",
            r#"{"all":{"routine":"r","token":"t"}}"#,
            r#"{"Bad":{"routine":"r","token":"t"}}"#,
            r#"{"a":{"routine":"r/../x","token":"t"}}"#,
            r#"{"a":{"routine":"r"}}"#,
            r#"{"a":{"routine":"r","token":"t\nX: y"}}"#,
        ] {
            assert!(WakeMap::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parse_errors_never_echo_tokens() {
        let err = WakeMap::parse(r#"{"a":{"routine":"r/x","token":"very-secret"}}"#).unwrap_err();
        assert!(!err.contains("very-secret"));
        let err = WakeMap::parse(r#"{"a":{"token":"very-secret""#).unwrap_err();
        assert!(!err.contains("very-secret"));
    }
}
