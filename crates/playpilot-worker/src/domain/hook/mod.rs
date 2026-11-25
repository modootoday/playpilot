//! Inbound GitHub webhooks on `POST /hooks/github`.
//!
//! A delivery is accepted only with a valid `X-Hub-Signature-256` under the
//! Worker secret; without the secret every delivery is refused (fail closed).
//! An accepted delivery becomes a CloudEvent whose `data` is a reduced,
//! allow-listed projection. Payload text is never stored, executed or
//! interpreted beyond picking those fields.

pub mod hmac;
pub mod projection;

use serde_json::{Value, json};

use super::time::rfc3339;

/// Largest body accepted, in bytes (GitHub caps payloads at 25 MB).
pub const MAX_BODY: usize = 1024 * 1024;

/// The request headers the check reads, as sent.
#[derive(Debug, Clone, Default)]
pub struct Delivery<'a> {
    pub event: Option<&'a str>,
    pub id: Option<&'a str>,
    pub signature: Option<&'a str>,
    pub content_type: Option<&'a str>,
}

/// Why a delivery was refused; becomes a problem response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub status: u16,
    pub title: &'static str,
    pub detail: &'static str,
}

const fn refuse(status: u16, title: &'static str, detail: &'static str) -> Refusal {
    Refusal {
        status,
        title,
        detail,
    }
}

pub const NO_SECRET: Refusal = refuse(
    503,
    "Service unavailable",
    "inbound webhooks are not configured",
);
pub const TOO_LARGE: Refusal = refuse(413, "Payload too large", "the body is larger than 1 MiB");
pub const BAD_SIGNATURE: Refusal = refuse(
    401,
    "Unauthorized",
    "missing or invalid X-Hub-Signature-256",
);

/// A verified delivery as a CloudEvent: dedupe id, type and compact JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    pub id: String,
    pub ty: String,
    pub json: String,
}

/// Checks that need no body: a secret is configured and the declared length
/// fits. Run before reading the body.
pub fn precheck(has_secret: bool, declared_len: Option<u64>) -> Result<(), Refusal> {
    if !has_secret {
        return Err(NO_SECRET);
    }
    if declared_len.is_some_and(|n| n > MAX_BODY as u64) {
        return Err(TOO_LARGE);
    }
    Ok(())
}

fn delivery_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn source(payload: &Value) -> String {
    if let Some(repo) = projection::repository(payload) {
        return format!("https://github.com/{repo}");
    }
    if let Some(org) = projection::organization(payload) {
        return format!("https://github.com/{org}");
    }
    if let Some(id) = projection::installation(payload) {
        return format!("https://github.com/settings/installations/{id}");
    }
    "https://github.com".into()
}

/// Verifies and converts one delivery. `secret` is the Worker secret;
/// `now` is the receipt time in Unix seconds.
pub fn receive(
    secret: Option<&str>,
    d: &Delivery,
    body: &[u8],
    now: u64,
) -> Result<Received, Refusal> {
    let secret = secret.filter(|s| !s.is_empty());
    precheck(secret.is_some(), Some(body.len() as u64))?;
    let secret = secret.ok_or(NO_SECRET)?;
    if !hmac::verify(secret.as_bytes(), body, d.signature) {
        return Err(BAD_SIGNATURE);
    }
    let media = d
        .content_type
        .and_then(|c| c.split(';').next())
        .map(|c| c.trim().to_ascii_lowercase());
    if media.as_deref() != Some("application/json") {
        return Err(refuse(
            415,
            "Unsupported media type",
            "set the webhook content type to application/json",
        ));
    }
    let id =
        d.id.map(str::trim)
            .filter(|s| delivery_id(s))
            .ok_or(refuse(
                400,
                "Bad request",
                "missing or invalid X-GitHub-Delivery",
            ))?;
    let event = d
        .event
        .map(str::trim)
        .filter(|s| projection::token(s))
        .ok_or(refuse(
            400,
            "Bad request",
            "missing or invalid X-GitHub-Event",
        ))?;
    let payload: Value = serde_json::from_slice(body)
        .ok()
        .filter(Value::is_object)
        .ok_or(refuse(400, "Bad request", "the body is not a JSON object"))?;
    let ty = match projection::action(&payload) {
        Some(action) => format!("com.github.{event}.{action}"),
        None => format!("com.github.{event}"),
    };
    let ev = json!({
        "specversion": "1.0",
        "id": id,
        "source": source(&payload),
        "type": ty,
        "time": rfc3339(now),
        "datacontenttype": "application/json",
        "data": projection::project(&payload),
    });
    Ok(Received {
        id: id.to_string(),
        ty,
        json: ev.to_string(),
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
