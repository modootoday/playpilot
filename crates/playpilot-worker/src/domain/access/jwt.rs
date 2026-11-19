//! The `Cf-Access-Jwt-Assertion` token split into what the checks need.
//! Only compact RS256 tokens with a key id are accepted.

use serde::Deserialize;
use serde_json::Value;

use super::Reject;
use super::base64url::decode;

/// Larger assertions are refused before any decoding.
pub const MAX_LEN: usize = 16 * 1024;

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claims {
    pub aud: Vec<String>,
    pub iss: String,
    pub exp: u64,
    pub nbf: Option<u64>,
    /// Set by Access to the service token client id for service-token logins.
    pub common_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kid: String,
    pub claims: Claims,
    /// `<header>.<payload>` as sent: the bytes the signature covers.
    pub signing_input: String,
    pub signature: Vec<u8>,
}

fn segment<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, Reject> {
    let raw = decode(text).ok_or(Reject::Malformed("segment is not base64url"))?;
    serde_json::from_slice(&raw).map_err(|_| Reject::Malformed("segment is not the expected JSON"))
}

fn seconds(v: Option<&Value>) -> Result<Option<u64>, Reject> {
    match v {
        None => Ok(None),
        Some(n) => n
            .as_u64()
            .map(Some)
            .ok_or(Reject::Malformed("time claim is not a whole number")),
    }
}

fn claims(v: &Value) -> Result<Claims, Reject> {
    let aud = match v.get("aud") {
        Some(Value::String(a)) => vec![a.clone()],
        Some(Value::Array(list)) => list
            .iter()
            .map(|a| a.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or(Reject::Malformed("aud holds a non-string"))?,
        _ => return Err(Reject::Malformed("aud is missing")),
    };
    let iss = v
        .get("iss")
        .and_then(Value::as_str)
        .ok_or(Reject::Malformed("iss is missing"))?;
    let exp = seconds(v.get("exp"))?.ok_or(Reject::Malformed("exp is missing"))?;
    let common_name = v
        .get("common_name")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
    Ok(Claims {
        aud,
        iss: iss.to_string(),
        exp,
        nbf: seconds(v.get("nbf"))?,
        common_name,
    })
}

pub fn parse(token: &str) -> Result<Token, Reject> {
    if token.len() > MAX_LEN {
        return Err(Reject::Malformed("token is too long"));
    }
    let mut parts = token.split('.');
    let (Some(head), Some(body), Some(sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Reject::Malformed("token is not three segments"));
    };
    let header: Header = segment(head)?;
    if header.alg != "RS256" {
        return Err(Reject::Algorithm);
    }
    let kid = header
        .kid
        .filter(|k| !k.is_empty())
        .ok_or(Reject::Malformed("kid is missing"))?;
    let payload: Value = segment(body)?;
    let signature = decode(sig)
        .filter(|s| !s.is_empty())
        .ok_or(Reject::Malformed("signature is not base64url"))?;
    Ok(Token {
        kid,
        claims: claims(&payload)?,
        signing_input: format!("{head}.{body}"),
        signature,
    })
}
