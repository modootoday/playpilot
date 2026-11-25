//! `Webhook` resources of the compiled config as delivery targets, and the
//! checks the Worker repeats at send time (the same rules as
//! `playpilot validate`): https only, no private or loopback hosts, type
//! prefixes on dot boundaries, secrets by binding name only.

use serde_json::Value;

use crate::domain::config::Compiled;
use crate::domain::eventlog::type_matches;
use crate::domain::ops::Switches;

/// Signing secrets are Worker secret bindings with this prefix.
pub const SECRET_PREFIX: &str = "PLAYPILOT_WEBHOOK_SECRET_";
/// A URL kept out of the config is a Worker secret binding with this prefix.
pub const URL_PREFIX: &str = "PLAYPILOT_WEBHOOK_URL_";

/// Where the target URL comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Url {
    Literal(String),
    /// The name of a Worker secret binding holding the URL.
    Secret(String),
}

/// One outbound `Webhook` resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    pub url: Url,
    /// The Worker secret binding holding `whsec_<base64>`.
    pub secret: String,
    /// Type prefixes; an event is sent when any matches.
    pub events: Vec<String>,
}

impl Target {
    pub fn wants(&self, ty: &str) -> bool {
        self.events.iter().any(|p| type_matches(ty, p))
    }

    /// Reads one resource's `spec`; `None` for inbound or invalid webhooks.
    pub fn from_spec(name: &str, spec: &Value) -> Option<Self> {
        if spec.get("direction").and_then(Value::as_str) == Some("inbound") {
            return None;
        }
        let url = match spec.get("url")? {
            Value::String(u) => Url::Literal(u.clone()),
            v => Url::Secret(v.get("secretRef")?.as_str()?.to_string()),
        };
        let signing = spec.get("signing")?;
        if signing.get("scheme").and_then(Value::as_str) != Some("standard-webhooks") {
            return None;
        }
        let secret = signing.get("secretRef")?.as_str()?.to_string();
        let events: Vec<String> = spec
            .get("events")?
            .as_array()?
            .iter()
            .map(|e| e.as_str().map(str::to_string))
            .collect::<Option<_>>()?;
        let t = Self {
            name: name.to_string(),
            url,
            secret,
            events,
        };
        check(&t).is_ok().then_some(t)
    }
}

/// Every valid outbound webhook not paused by file or Worker `ops`.
pub fn targets(config: &Compiled, switches: &Switches) -> Vec<Target> {
    config
        .resources()
        .filter(|(kind, _, _)| *kind == "Webhook")
        .filter(|(_, name, _)| !switches.effective("Webhook", name).is_paused())
        .filter_map(|(_, name, r)| Target::from_spec(name, r.get("spec")?))
        .collect()
}

/// Names of the targets that want an event of type `ty`.
pub fn matching(targets: &[Target], ty: &str) -> Vec<String> {
    targets
        .iter()
        .filter(|t| t.wants(ty))
        .map(|t| t.name.clone())
        .collect()
}

/// A type prefix: dot-separated parts of `[a-z0-9_-]`, no empty part.
pub fn valid_prefix(p: &str) -> bool {
    p.len() <= 200
        && p.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
        })
}

fn binding(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    })
}

/// The config-time checks for one target.
pub fn check(t: &Target) -> Result<(), String> {
    match &t.url {
        Url::Literal(u) => check_url(u)?,
        Url::Secret(name) if !binding(name, URL_PREFIX) => {
            return Err(format!("url secretRef must be {URL_PREFIX}<NAME>"));
        }
        Url::Secret(_) => {}
    }
    if !binding(&t.secret, SECRET_PREFIX) {
        return Err(format!("signing.secretRef must be {SECRET_PREFIX}<NAME>"));
    }
    if t.events.is_empty() {
        return Err("events must name at least one type prefix".into());
    }
    if let Some(bad) = t.events.iter().find(|e| !valid_prefix(e)) {
        return Err(format!("`{bad}` is not a dotted type prefix"));
    }
    Ok(())
}

fn private_v4(o: [u8; 4]) -> bool {
    matches!(
        o,
        [0 | 10 | 127, ..]
            | [100, 64..=127, ..]
            | [169, 254, ..]
            | [172, 16..=31, ..]
            | [192, 168, ..]
            | [192, 0, 0, _]
            | [198, 18..=19, ..]
            | [224..=255, ..]
    )
}

fn dotted_v4(host: &str) -> Option<[u8; 4]> {
    let parts: Vec<u8> = host
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    parts.try_into().ok()
}

/// An `https://` URL whose host is a public DNS name or public IPv4 address:
/// no user info, no IPv6 literals, no loopback, private, link-local or
/// single-label hosts, no `localhost`, `.local`, `.internal` or `.home.arpa`.
pub fn check_url(url: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("webhook url {why}"));
    let Some(rest) = url.strip_prefix("https://") else {
        return bad("must start with https://");
    };
    if url.len() > 2_048 || !url.bytes().all(|c| c.is_ascii_graphic()) {
        return bad("must be printable ASCII without spaces");
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return bad("must not carry user info");
    }
    if authority.starts_with('[') {
        return bad("must not use an IPv6 literal");
    }
    let host = match authority.rsplit_once(':') {
        Some((h, port)) if port.bytes().all(|b| b.is_ascii_digit()) && !port.is_empty() => h,
        Some(_) => return bad("has an invalid port"),
        None => authority,
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return bad("has no host");
    }
    if let Some(ip) = dotted_v4(&host) {
        return if private_v4(ip) {
            bad("must not point at a private, loopback or reserved address")
        } else {
            Ok(())
        };
    }
    let label_ok = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    if !host.split('.').all(label_ok) {
        return bad("has an invalid host name");
    }
    let local = !host.contains('.')
        || host == "localhost"
        || [".localhost", ".local", ".internal", ".home.arpa", ".lan"]
            .iter()
            .any(|s| host.ends_with(s));
    if local {
        return bad("must not point at a local host name");
    }
    if host
        .split('.')
        .next_back()
        .is_some_and(|tld| tld.bytes().all(|b| b.is_ascii_digit()))
    {
        return bad("has an invalid host name");
    }
    Ok(())
}
