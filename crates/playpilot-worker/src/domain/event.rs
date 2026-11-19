//! CloudEvents 1.0 structured JSON validation for `POST /v1/events`.
//! Events are data: they are checked, stored and fanned out, never executed.

use serde_json::Value;

use super::time::is_rfc3339;

pub const MAX_BATCH: usize = 100;
pub const MAX_BODY: usize = 256 * 1024;

const CONTEXT: [&str; 8] = [
    "specversion",
    "id",
    "source",
    "type",
    "subject",
    "time",
    "datacontenttype",
    "dataschema",
];

/// A validated event: its dedupe key (source and id) and compact JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub key: String,
    pub json: String,
}

/// `fleet.<noun>.<verb>`, each part lower-case words joined by hyphens.
pub fn valid_type(t: &str) -> bool {
    let parts: Vec<&str> = t.split('.').collect();
    let word = |p: &str| {
        !p.is_empty()
            && p.split('-')
                .all(|w| !w.is_empty() && w.bytes().all(|c| c.is_ascii_lowercase()))
    };
    parts.len() == 3 && parts[0] == "fleet" && word(parts[1]) && word(parts[2])
}

fn uri_reference(s: &str) -> bool {
    !s.is_empty() && s.len() <= 2_048 && s.bytes().all(|c| c.is_ascii_graphic())
}

fn absolute_uri(s: &str) -> bool {
    uri_reference(s)
        && s.split_once(':').is_some_and(|(scheme, _)| {
            scheme
                .bytes()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c))
        })
}

fn text<'v>(obj: &'v serde_json::Map<String, Value>, key: &str) -> Result<Option<&'v str>, String> {
    match obj.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(format!("`{key}` must be a string")),
    }
}

/// Validates one event object.
pub fn validate(v: &Value) -> Result<Accepted, String> {
    let obj = v.as_object().ok_or("an event must be a JSON object")?;
    if text(obj, "specversion")? != Some("1.0") {
        return Err("specversion must be \"1.0\"".into());
    }
    let id = text(obj, "id")?
        .filter(|s| !s.is_empty())
        .ok_or("`id` is required")?;
    if id.len() > 256 {
        return Err("`id` is longer than 256 bytes".into());
    }
    let source = text(obj, "source")?.ok_or("`source` is required")?;
    if !uri_reference(source) {
        return Err("`source` must be a URI reference".into());
    }
    let ty = text(obj, "type")?.ok_or("`type` is required")?;
    if !valid_type(ty) {
        return Err(format!("`{ty}` is not fleet.<noun>.<verb>"));
    }
    if let Some(t) = text(obj, "time")?
        && !is_rfc3339(t)
    {
        return Err("`time` must be an RFC 3339 date-time".into());
    }
    if let Some(s) = text(obj, "dataschema")?
        && !absolute_uri(s)
    {
        return Err("`dataschema` must be an absolute URI".into());
    }
    text(obj, "subject")?;
    text(obj, "datacontenttype")?;
    for key in obj
        .keys()
        .filter(|k| !CONTEXT.contains(&k.as_str()) && *k != "data")
    {
        let ok = !key.is_empty()
            && key.len() <= 20
            && key
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if !ok {
            return Err(format!(
                "extension attribute `{key}` is not lower-case alphanumeric"
            ));
        }
    }
    Ok(Accepted {
        key: format!("{source} {id}"),
        json: v.to_string(),
    })
}

/// Parses a structured (`application/cloudevents+json`) or batch
/// (`application/cloudevents-batch+json`) body. All events must be valid.
pub fn parse(content_type: &str, body: &str) -> Result<Vec<Accepted>, String> {
    if body.len() > MAX_BODY {
        return Err(format!("the body is larger than {MAX_BODY} bytes"));
    }
    let media = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let v: Value = serde_json::from_str(body).map_err(|e| format!("invalid JSON: {e}"))?;
    match media.as_str() {
        "application/cloudevents+json" => Ok(vec![validate(&v)?]),
        "application/cloudevents-batch+json" => {
            let items = v.as_array().ok_or("a batch must be a JSON array")?;
            if items.is_empty() || items.len() > MAX_BATCH {
                return Err(format!("a batch holds 1 to {MAX_BATCH} events"));
            }
            items
                .iter()
                .enumerate()
                .map(|(i, e)| validate(e).map_err(|m| format!("event {i}: {m}")))
                .collect()
        }
        other => Err(format!("unsupported content type `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn ev() -> Value {
        json!({
            "specversion": "1.0",
            "id": "e-1",
            "source": "/nodes/node-a",
            "type": "fleet.run.finished",
            "time": "2026-11-19T10:00:00Z",
            "data": {"command": "rm -rf /"}
        })
    }

    #[test]
    fn accepts_a_valid_event_and_keys_by_source_and_id() {
        let a = validate(&ev()).unwrap();
        assert_eq!(a.key, "/nodes/node-a e-1");
        assert!(a.json.contains("fleet.run.finished"));
    }

    #[test]
    fn refuses_bad_context_attributes() {
        let cases: [(&str, Value); 8] = [
            ("specversion", json!("0.3")),
            ("id", json!("")),
            ("source", json!("has space")),
            ("type", json!("run.finished")),
            ("type", json!("fleet.Run.finished")),
            ("time", json!("yesterday")),
            ("dataschema", json!("relative/path")),
            ("subject", json!(5)),
        ];
        for (key, value) in cases {
            let mut e = ev();
            e[key] = value;
            assert!(validate(&e).is_err(), "{key}");
        }
        let mut e = ev();
        e.as_object_mut().unwrap().remove("source");
        assert!(validate(&e).is_err());
        let mut e = ev();
        e["Bad-Ext"] = json!("x");
        assert!(validate(&e).is_err());
        let mut e = ev();
        e["traceparent"] = json!("x");
        assert!(validate(&e).is_ok());
    }

    #[test]
    fn types() {
        assert!(valid_type("fleet.claim.created"));
        assert!(valid_type("fleet.config-set.applied"));
        assert!(!valid_type("fleet.claim"));
        assert!(!valid_type("fleet.claim.created.now"));
        assert!(!valid_type("other.claim.created"));
    }

    #[test]
    fn parses_single_and_batch() {
        let one = ev().to_string();
        assert_eq!(
            parse("application/cloudevents+json; charset=utf-8", &one)
                .unwrap()
                .len(),
            1
        );
        let batch = json!([ev(), ev()]).to_string();
        assert_eq!(
            parse("application/cloudevents-batch+json", &batch)
                .unwrap()
                .len(),
            2
        );
        assert!(parse("application/json", &one).is_err());
        assert!(parse("application/cloudevents-batch+json", "[]").is_err());
        let mut bad = ev();
        bad["type"] = json!("x");
        let mixed = json!([ev(), bad]).to_string();
        let err = parse("application/cloudevents-batch+json", &mixed).unwrap_err();
        assert!(err.starts_with("event 1:"));
        assert!(parse("application/cloudevents+json", "{").is_err());
    }
}
