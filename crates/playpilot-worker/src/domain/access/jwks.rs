//! The team's signing keys (`/cdn-cgi/access/certs`) and their per-isolate
//! cache. Only RSA signing keys of at least 2048 bits are kept.

use serde_json::Value;

use super::base64url::decode;

/// How long fetched keys are trusted before they are fetched again.
pub const TTL: u64 = 10 * 60;
/// An unknown key id refetches only when the cache is at least this old,
/// so forged key ids cannot make every request fetch.
pub const REFETCH_AFTER: u64 = 30;
const MIN_MODULUS_BYTES: usize = 256;

/// One RSA public key; `n` and `e` stay base64url for a JWK import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jwk {
    pub kid: String,
    pub n: String,
    pub e: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Jwks(Vec<Jwk>);

fn usable(k: &Value) -> Option<Jwk> {
    let s = |name: &str| k.get(name).and_then(Value::as_str);
    if s("kty") != Some("RSA") {
        return None;
    }
    if s("use").is_some_and(|u| u != "sig") || s("alg").is_some_and(|a| a != "RS256") {
        return None;
    }
    let (kid, n, e) = (s("kid")?, s("n")?, s("e")?);
    let modulus = decode(n)?;
    let significant = modulus.iter().skip_while(|b| **b == 0).count();
    let exponent = decode(e)?;
    if kid.is_empty() || significant < MIN_MODULUS_BYTES || exponent.is_empty() {
        return None;
    }
    Some(Jwk {
        kid: kid.into(),
        n: n.into(),
        e: e.into(),
    })
}

impl Jwks {
    /// `{"keys": [...]}`; keys that are not usable RSA signing keys are skipped.
    pub fn parse(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("jwks: {e}"))?;
        let keys = v
            .get("keys")
            .and_then(Value::as_array)
            .ok_or("jwks: `keys` must be an array")?;
        let keys: Vec<Jwk> = keys.iter().filter_map(usable).collect();
        if keys.is_empty() {
            return Err("jwks: no usable RSA signing key".into());
        }
        Ok(Self(keys))
    }

    pub fn find(&self, kid: &str) -> Option<&Jwk> {
        self.0.iter().find(|k| k.kid == kid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Hit(Jwk),
    /// Fetch the keys, `store` them and look up again.
    Fetch,
    /// The cache is fresh and does not know this key id.
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub struct KeyCache {
    entry: Option<(String, u64, Jwks)>,
}

impl KeyCache {
    pub fn lookup(&self, url: &str, kid: &str, now: u64) -> Lookup {
        let Some((cached_url, at, keys)) = &self.entry else {
            return Lookup::Fetch;
        };
        let age = now.saturating_sub(*at);
        if cached_url != url || age >= TTL || now < *at {
            return Lookup::Fetch;
        }
        match keys.find(kid) {
            Some(k) => Lookup::Hit(k.clone()),
            None if age >= REFETCH_AFTER => Lookup::Fetch,
            None => Lookup::Unknown,
        }
    }

    pub fn store(&mut self, url: &str, now: u64, keys: Jwks) {
        self.entry = Some((url.to_string(), now, keys));
    }

    /// After a store, a key id the fresh keys lack is unknown, not refetched.
    pub fn after_fetch(&self, url: &str, kid: &str, now: u64) -> Lookup {
        match self.lookup(url, kid, now) {
            Lookup::Fetch => Lookup::Unknown,
            other => other,
        }
    }
}
