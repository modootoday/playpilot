//! The allow-listed fields a stored GitHub delivery keeps. Everything is
//! rebuilt from scratch, so unknown fields and free text (titles, bodies,
//! comments, commit messages) never reach storage.

use serde_json::{Map, Value, json};

const MAX_LABELS: usize = 100;

fn repo_name(s: &str) -> bool {
    let parts: Vec<&str> = s.split('/').collect();
    parts.len() == 2
        && s.len() <= 140
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
}

/// A GitHub login, or an app's `name[bot]`.
pub fn login(s: &str) -> bool {
    let base = s.strip_suffix("[bot]").unwrap_or(s);
    !base.is_empty()
        && base.len() <= 39
        && base
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}

/// Event names and actions: `^[a-z_]{1,64}$`.
pub fn token(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

fn git_ref(s: &str) -> bool {
    !s.is_empty() && s.len() <= 255 && s.bytes().all(|b| b.is_ascii_graphic())
}

fn sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn label(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && !s.chars().any(char::is_control)
}

fn at<'v>(v: &'v Value, path: &[&str]) -> Option<&'v Value> {
    path.iter().try_fold(v, |cur, k| cur.get(k))
}

fn str_at<'v>(v: &'v Value, path: &[&str], ok: fn(&str) -> bool) -> Option<&'v str> {
    at(v, path).and_then(Value::as_str).filter(|s| ok(s))
}

fn first_str<'v>(v: &'v Value, paths: &[&[&str]], ok: fn(&str) -> bool) -> Option<&'v str> {
    paths.iter().find_map(|p| str_at(v, p, ok))
}

pub fn repository(payload: &Value) -> Option<&str> {
    str_at(payload, &["repository", "full_name"], repo_name)
}

pub fn organization(payload: &Value) -> Option<&str> {
    str_at(payload, &["organization", "login"], login)
}

pub fn installation(payload: &Value) -> Option<u64> {
    at(payload, &["installation", "id"]).and_then(Value::as_u64)
}

pub fn action(payload: &Value) -> Option<&str> {
    str_at(payload, &["action"], token)
}

/// The reduced `data` of the CloudEvent.
pub fn project(payload: &Value) -> Value {
    let mut out = Map::new();
    let mut put = |k: &str, v: Option<Value>| {
        if let Some(v) = v {
            out.insert(k.into(), v);
        }
    };
    put("repository", repository(payload).map(Value::from));
    put(
        "number",
        [
            &["number"][..],
            &["issue", "number"],
            &["pull_request", "number"],
        ]
        .iter()
        .find_map(|p| at(payload, p).and_then(Value::as_u64))
        .map(Value::from),
    );
    put("action", action(payload).map(Value::from));
    put(
        "sender",
        str_at(payload, &["sender", "login"], login).map(Value::from),
    );
    put("ref", str_at(payload, &["ref"], git_ref).map(Value::from));
    put(
        "sha",
        first_str(
            payload,
            &[
                &["after"],
                &["pull_request", "head", "sha"],
                &["check_run", "head_sha"],
                &["check_suite", "head_sha"],
                &["workflow_run", "head_sha"],
                &["sha"],
            ],
            sha,
        )
        .map(|s| Value::from(s.to_ascii_lowercase())),
    );
    let labels: Vec<Value> = [&["pull_request", "labels"][..], &["issue", "labels"]]
        .iter()
        .find_map(|p| at(payload, p).and_then(Value::as_array))
        .map(|list| {
            list.iter()
                .filter_map(|l| l.get("name").and_then(Value::as_str))
                .filter(|n| label(n))
                .take(MAX_LABELS)
                .map(Value::from)
                .collect()
        })
        .unwrap_or_default();
    put("labels", (!labels.is_empty()).then(|| json!(labels)));
    put(
        "state",
        first_str(
            payload,
            &[&["pull_request", "state"], &["issue", "state"], &["state"]],
            token,
        )
        .map(Value::from),
    );
    Value::Object(out)
}
