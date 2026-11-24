//! Domain: the public `/v1/state` subset (`PublicState` in
//! `schemas/worker.v1.openapi.yaml`). A field outside the subset is refused,
//! never dropped, so a Worker that starts leaking is noticed (G8).

use serde_json::{Map, Value, json};

const TOP: &[&str] = &["generatedAt", "nodes", "lanes", "projects"];
const NODE: &[&str] = &["id", "status", "lastSeen"];
const LANE: &[&str] = &["name", "paused", "queued", "leased"];
const PROJECT: &[&str] = &["name", "lastRun"];
const STATUSES: &[&str] = &["online", "offline", "paused"];
const MAX_LIST: usize = 1024;

/// Validates `/v1/state` JSON and returns it rebuilt from the public fields.
pub fn filter(text: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let top = object(&v, "state", TOP)?;
    let mut out = Map::new();
    out.insert(
        "generatedAt".into(),
        json!(datetime(top.get("generatedAt"), "generatedAt")?),
    );
    out.insert("nodes".into(), list(top.get("nodes"), "nodes", node)?);
    out.insert("lanes".into(), list(top.get("lanes"), "lanes", lane)?);
    if let Some(p) = top.get("projects") {
        out.insert("projects".into(), list(Some(p), "projects", project)?);
    }
    Ok(Value::Object(out))
}

fn object<'a>(v: &'a Value, at: &str, allowed: &[&str]) -> Result<&'a Map<String, Value>, String> {
    let obj = v.as_object().ok_or(format!("{at}: expected an object"))?;
    if let Some(k) = obj.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(format!("{at}: `{k}` is not in the public subset"));
    }
    Ok(obj)
}

fn list(
    v: Option<&Value>,
    at: &str,
    item: fn(&Value, &str) -> Result<Value, String>,
) -> Result<Value, String> {
    let arr = v
        .ok_or(format!("{at} is required"))?
        .as_array()
        .ok_or(format!("{at}: expected an array"))?;
    if arr.len() > MAX_LIST {
        return Err(format!("{at}: more than {MAX_LIST} entries"));
    }
    arr.iter()
        .enumerate()
        .map(|(i, x)| item(x, &format!("{at}[{i}]")))
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

fn node(v: &Value, at: &str) -> Result<Value, String> {
    let o = object(v, at, NODE)?;
    let id = string(o.get("id"), &format!("{at}.id"))?;
    if !valid_name(id) {
        return Err(format!("{at}.id: not an opaque node id"));
    }
    let status = string(o.get("status"), &format!("{at}.status"))?;
    if !STATUSES.contains(&status) {
        return Err(format!("{at}.status: `{status}` is not a status"));
    }
    let mut out = Map::new();
    out.insert("id".into(), json!(id));
    out.insert("status".into(), json!(status));
    if let Some(s) = o.get("lastSeen") {
        out.insert(
            "lastSeen".into(),
            json!(datetime(Some(s), &format!("{at}.lastSeen"))?),
        );
    }
    Ok(Value::Object(out))
}

fn lane(v: &Value, at: &str) -> Result<Value, String> {
    let o = object(v, at, LANE)?;
    let name = string(o.get("name"), &format!("{at}.name"))?;
    if !valid_lane(name) {
        return Err(format!("{at}.name: not a lane name"));
    }
    let mut out = Map::new();
    out.insert("name".into(), json!(name));
    if let Some(p) = o.get("paused") {
        let p = p
            .as_bool()
            .ok_or(format!("{at}.paused: expected a boolean"))?;
        out.insert("paused".into(), json!(p));
    }
    for key in ["queued", "leased"] {
        let n = o
            .get(key)
            .ok_or(format!("{at}.{key} is required"))?
            .as_u64()
            .ok_or(format!("{at}.{key}: expected a count"))?;
        out.insert(key.into(), json!(n));
    }
    Ok(Value::Object(out))
}

fn project(v: &Value, at: &str) -> Result<Value, String> {
    let o = object(v, at, PROJECT)?;
    let name = string(o.get("name"), &format!("{at}.name"))?;
    if !valid_name(name) {
        return Err(format!("{at}.name: not a project name"));
    }
    let mut out = Map::new();
    out.insert("name".into(), json!(name));
    if let Some(r) = o.get("lastRun") {
        out.insert(
            "lastRun".into(),
            json!(datetime(Some(r), &format!("{at}.lastRun"))?),
        );
    }
    Ok(Value::Object(out))
}

fn string<'a>(v: Option<&'a Value>, at: &str) -> Result<&'a str, String> {
    v.ok_or(format!("{at} is required"))?
        .as_str()
        .ok_or(format!("{at}: expected a string"))
}

/// RFC 3339 `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)`.
fn datetime<'a>(v: Option<&'a Value>, at: &str) -> Result<&'a str, String> {
    let s = string(v, at)?;
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    let head = b.len() >= 20
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && matches!(b[10], b'T' | b't')
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19);
    let mut rest = if head { &s[19..] } else { "" };
    if let Some(frac) = rest.strip_prefix('.') {
        let n = frac.bytes().take_while(u8::is_ascii_digit).count();
        rest = if n > 0 { &frac[n..] } else { "" };
    }
    let zone = rest == "Z"
        || rest == "z"
        || (rest.len() == 6
            && matches!(rest.as_bytes()[0], b'+' | b'-')
            && rest.as_bytes()[3] == b':'
            && rest
                .bytes()
                .enumerate()
                .all(|(i, c)| matches!(i, 0 | 3) || c.is_ascii_digit()));
    if head && zone {
        Ok(s)
    } else {
        Err(format!("{at}: not an RFC 3339 date-time"))
    }
}

/// `NodeId` in the OpenAPI: lower-case words joined by single hyphens.
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.split('-').all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

fn valid_lane(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"-_.".contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = r#"{
      "generatedAt": "2026-11-24T10:00:00Z",
      "nodes": [
        {"id": "node-a", "status": "online", "lastSeen": "2026-11-24T09:59:00Z"},
        {"id": "node-b", "status": "paused"}
      ],
      "lanes": [{"name": "build", "paused": false, "queued": 0, "leased": 2}],
      "projects": [{"name": "example", "lastRun": "2026-11-24T08:00:00.5+09:00"}]
    }"#;

    #[test]
    fn keeps_the_public_subset() {
        let v = filter(OK).unwrap();
        assert_eq!(v["nodes"][0]["id"], "node-a");
        assert_eq!(v["nodes"][1].as_object().unwrap().len(), 2);
        assert_eq!(v["lanes"][0]["leased"], 2);
        assert_eq!(v["projects"][0]["name"], "example");
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["generatedAt", "nodes", "lanes", "projects"]);
    }

    #[test]
    fn projects_are_optional() {
        let v = filter(r#"{"generatedAt":"2026-11-24T10:00:00Z","nodes":[],"lanes":[]}"#).unwrap();
        assert!(v.get("projects").is_none());
    }

    #[test]
    fn refuses_fields_outside_the_subset() {
        for (from, to) in [
            (r#""generatedAt""#, r#""device": "x", "generatedAt""#),
            (
                r#""id": "node-a","#,
                r#""id": "node-a", "device": "laptop","#,
            ),
            (
                r#""status": "paused""#,
                r#""status": "paused", "rev": "abc""#,
            ),
            (r#""queued": 0"#, r#""queued": 0, "items": []"#),
            (
                r#""name": "example""#,
                r#""name": "example", "repo": "a/b""#,
            ),
        ] {
            let text = OK.replacen(from, to, 1);
            assert_ne!(text, OK, "{to}");
            let e = filter(&text).unwrap_err();
            assert!(e.contains("not in the public subset"), "{e}");
        }
    }

    #[test]
    fn refuses_malformed_values() {
        for (from, to) in [
            (r#""node-a""#, r#""Node A""#),
            (r#""online""#, r#""busy""#),
            (r#""leased": 2"#, r#""leased": -1"#),
            (r#""paused": false"#, r#""paused": "no""#),
            (r#""2026-11-24T10:00:00Z""#, r#""yesterday""#),
            (r#""example""#, r#""someone@example.com""#),
            (
                r#""lanes": [{"name": "build", "paused": false, "queued": 0, "leased": 2}]"#,
                r#""lanes": {}"#,
            ),
        ] {
            let text = OK.replacen(from, to, 1);
            assert_ne!(text, OK, "{to}");
            assert!(filter(&text).is_err(), "{to}");
        }
        assert!(filter(r#"{"nodes":[],"lanes":[]}"#).is_err());
        assert!(filter("[]").is_err());
        assert!(filter("not json").is_err());
    }

    #[test]
    fn datetimes() {
        let ok = [
            "2026-11-24T10:00:00Z",
            "2026-11-24T10:00:00.123Z",
            "2026-11-24T10:00:00-05:00",
        ];
        for s in ok {
            assert!(datetime(Some(&json!(s)), "t").is_ok(), "{s}");
        }
        let bad = [
            "2026-11-24",
            "2026-11-24T10:00:00",
            "2026-11-24T10:00:00.Z",
            "2026-11-24T10:00:00+0500",
        ];
        for s in bad {
            assert!(datetime(Some(&json!(s)), "t").is_err(), "{s}");
        }
    }

    #[test]
    fn fields_match_the_openapi_subset() {
        let api = include_str!("../../../../schemas/worker.v1.openapi.yaml");
        let start = api.find("    PublicState:").unwrap();
        let end = start + api[start..].find("    Problem:").unwrap();
        let section = &api[start..end];
        for field in TOP.iter().chain(NODE).chain(LANE).chain(PROJECT) {
            assert!(section.contains(&format!(" {field}:")), "{field}");
        }
        for s in STATUSES {
            assert!(section.contains(s), "{s}");
        }
    }
}
