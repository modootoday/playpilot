//! Outbound `Webhook` checks. The Worker repeats the same rules at send time
//! (`playpilot-worker` `domain::outbound::target`): an https URL to a public
//! host or a `PLAYPILOT_WEBHOOK_URL_*` binding, a Standard Webhooks signing
//! secret named `PLAYPILOT_WEBHOOK_SECRET_*`, and type prefixes on dot
//! boundaries.

use toml::{Table, Value};

pub const SECRET_PREFIX: &str = "PLAYPILOT_WEBHOOK_SECRET_";
pub const URL_PREFIX: &str = "PLAYPILOT_WEBHOOK_URL_";

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

/// An `https://` URL whose host is a public DNS name or public IPv4 address.
pub fn check_url(url: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("url {why}"));
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
        Some((h, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => h,
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
        .rsplit('.')
        .next()
        .is_some_and(|tld| tld.bytes().all(|b| b.is_ascii_digit()))
    {
        return bad("has an invalid host name");
    }
    Ok(())
}

/// Problems with one Webhook `spec`; inbound webhooks are not checked here.
pub fn check(spec: &Table) -> Vec<String> {
    let mut out = Vec::new();
    match spec.get("direction").and_then(Value::as_str) {
        Some("inbound") => return out,
        None | Some("outbound") => {}
        Some(other) => out.push(format!("direction \"{other}\" is not outbound or inbound")),
    }
    match spec.get("url") {
        Some(Value::String(u)) => {
            if let Err(e) = check_url(u) {
                out.push(e);
            }
        }
        Some(Value::Table(t)) => match t.get("secretRef").and_then(Value::as_str) {
            Some(n) if binding(n, URL_PREFIX) => {}
            _ => out.push(format!("url secretRef must be {URL_PREFIX}<NAME>")),
        },
        _ => out.push("url is required (https URL or { secretRef })".into()),
    }
    let signing = spec.get("signing").and_then(Value::as_table);
    if signing
        .and_then(|s| s.get("scheme"))
        .and_then(Value::as_str)
        != Some("standard-webhooks")
    {
        out.push("signing.scheme must be \"standard-webhooks\"".into());
    }
    match signing
        .and_then(|s| s.get("secretRef"))
        .and_then(Value::as_str)
    {
        Some(n) if binding(n, SECRET_PREFIX) => {}
        _ => out.push(format!("signing.secretRef must be {SECRET_PREFIX}<NAME>")),
    }
    match spec.get("events").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => {
            for e in list {
                match e.as_str() {
                    Some(p) if valid_prefix(p) => {}
                    _ => out.push(format!("events: {e} is not a dotted type prefix")),
                }
            }
        }
        _ => out.push("events must list at least one type prefix".into()),
    }
    out
}
