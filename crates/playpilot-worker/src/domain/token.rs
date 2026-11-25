//! Bearer tokens for `/mcp`, which is served on a host without Cloudflare
//! Access. The binding maps the SHA-256 of each token to the actors it may
//! speak for; the plain token is never stored. One token may stand for several
//! actors (cloud sessions share one environment), so a call names its actor
//! with the `as` argument, checked against that list.

use std::collections::BTreeMap;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::identity::{Identity, IdentityMap};
use super::ids::valid_name;

/// What one token allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// Sorted, unique, never empty.
    pub actors: Vec<String>,
    /// Lease holder id; defaults to the actor of each call.
    pub claim: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenMap(BTreeMap<String, Grant>);

/// Lower-case hex SHA-256 of `token`.
pub fn sha256_hex(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The token of an `Authorization: Bearer <token>` header value.
pub fn bearer(header: Option<&str>) -> Option<&str> {
    let (scheme, token) = header?.trim().split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

impl TokenMap {
    /// `{"<sha256 hex>": {"actors": ["a", "b"], "claim": "node-a"}}`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("tokens: {e}"))?;
        let obj = v.as_object().ok_or("tokens must be a JSON object")?;
        let mut out = BTreeMap::new();
        for (hash, entry) in obj {
            if !is_hash(hash) {
                return Err("tokens: keys are lower-case hex SHA-256 digests".into());
            }
            let entry = entry
                .as_object()
                .ok_or("tokens: each value is {actors, claim}")?;
            if let Some(k) = entry.keys().find(|k| *k != "actors" && *k != "claim") {
                return Err(format!("tokens: unknown key `{k}`"));
            }
            let mut actors: Vec<String> = entry
                .get("actors")
                .and_then(Value::as_array)
                .ok_or("tokens: `actors` must be a list")?
                .iter()
                .map(|a| a.as_str().map(str::to_string))
                .collect::<Option<_>>()
                .ok_or("tokens: actors are strings")?;
            actors.sort();
            actors.dedup();
            if actors.is_empty() {
                return Err("tokens: `actors` must not be empty".into());
            }
            if let Some(bad) = actors.iter().find(|a| !valid_name(a) || *a == "all") {
                return Err(format!("tokens: `{bad}` is not a valid actor name"));
            }
            let claim = match entry.get("claim") {
                None | Some(Value::Null) => None,
                Some(Value::String(c)) if valid_name(c) => Some(c.clone()),
                Some(_) => return Err("tokens: `claim` must be a valid name".into()),
            };
            out.insert(hash.clone(), Grant { actors, claim });
        }
        Ok(Self(out))
    }

    /// The grant for a presented token. Looking up a hash needs no
    /// constant-time comparison: the key is not the secret.
    pub fn authenticate(&self, token: &str) -> Option<&Grant> {
        self.0.get(&sha256_hex(token))
    }

    /// The grant for an `Authorization` header value; `None` for a missing,
    /// malformed or unknown bearer token. Every `/mcp` tool needs one.
    pub fn grant_for(&self, header: Option<&str>) -> Option<&Grant> {
        self.authenticate(bearer(header)?)
    }

    /// Every actor any token may speak for, sorted and unique.
    pub fn actors(&self) -> Vec<String> {
        let mut a: Vec<String> = self.0.values().flat_map(|g| g.actors.clone()).collect();
        a.sort();
        a.dedup();
        a
    }
}

/// Who is calling `/mcp`: an Access identity (one actor) or a token grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    Access(Identity),
    Token(Grant),
}

impl Caller {
    pub fn actors(&self) -> Vec<String> {
        match self {
            Caller::Access(id) => vec![id.actor.clone()],
            Caller::Token(g) => g.actors.clone(),
        }
    }

    /// The identity for one call. `as` is required when the caller may speak
    /// for several actors and must be one of them.
    pub fn resolve(&self, as_actor: Option<&str>) -> Result<Identity, String> {
        let actors = self.actors();
        let actor = match (as_actor, actors.as_slice()) {
            (Some(a), _) if actors.iter().any(|x| x == a) => a.to_string(),
            (Some(a), _) => {
                return Err(format!(
                    "`as`: this credential may not act as `{a}` (allowed: {})",
                    actors.join(", ")
                ));
            }
            (None, [only]) => only.clone(),
            (None, _) => {
                return Err(format!(
                    "`as` is required: this credential stands for {}",
                    actors.join(", ")
                ));
            }
        };
        let claim = match self {
            Caller::Access(id) => id.claim.clone(),
            Caller::Token(g) => g.claim.clone().unwrap_or_else(|| actor.clone()),
        };
        Ok(Identity { actor, claim })
    }
}

/// Actors a broadcast reaches: every actor named by either binding.
pub fn directory(identities: Option<&IdentityMap>, tokens: Option<&TokenMap>) -> Vec<String> {
    let mut a: Vec<String> = identities
        .map(IdentityMap::actors)
        .unwrap_or_default()
        .into_iter()
        .chain(tokens.map(TokenMap::actors).unwrap_or_default())
        .collect();
    a.sort();
    a.dedup();
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn map() -> TokenMap {
        let shared = sha256_hex("shared-token");
        TokenMap::parse(&format!(
            r#"{{"{ABC}":{{"actors":["solo"]}},"{shared}":{{"actors":["beta","alpha","beta"],"claim":"cloud"}}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn hashes_like_sha256() {
        assert_eq!(sha256_hex("abc"), ABC);
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn reads_bearer_headers() {
        assert_eq!(bearer(Some("Bearer abc")), Some("abc"));
        assert_eq!(bearer(Some("bearer  abc ")), Some("abc"));
        assert_eq!(bearer(Some("Basic abc")), None);
        assert_eq!(bearer(Some("Bearer ")), None);
        assert_eq!(bearer(Some("Bearer")), None);
        assert_eq!(bearer(None), None);
    }

    #[test]
    fn authenticates_by_hash() {
        let m = map();
        let solo = m.authenticate("abc").unwrap();
        assert_eq!(solo.actors, ["solo"]);
        let shared = m.authenticate("shared-token").unwrap();
        assert_eq!(shared.actors, ["alpha", "beta"]);
        assert!(m.authenticate("ABC").is_none());
        assert!(m.authenticate(ABC).is_none());
        assert_eq!(m.actors(), ["alpha", "beta", "solo"]);
    }

    #[test]
    fn refuses_bad_maps() {
        for bad in [
            "[]".to_string(),
            r#"{"abc":{"actors":["a"]}}"#.into(),
            format!(r#"{{"{}":{{"actors":["a"]}}}}"#, ABC.to_uppercase()),
            format!(r#"{{"{ABC}":{{"actors":[]}}}}"#),
            format!(r#"{{"{ABC}":{{"actors":["all"]}}}}"#),
            format!(r#"{{"{ABC}":{{"actors":["Bad Name"]}}}}"#),
            format!(r#"{{"{ABC}":{{"actors":["a"],"claim":5}}}}"#),
            format!(r#"{{"{ABC}":{{"actors":["a"],"extra":1}}}}"#),
            format!(r#"{{"{ABC}":"a"}}"#),
        ] {
            assert!(TokenMap::parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn resolves_the_actor_of_a_call() {
        let m = map();
        let solo = Caller::Token(m.authenticate("abc").unwrap().clone());
        let id = solo.resolve(None).unwrap();
        assert_eq!((id.actor.as_str(), id.claim.as_str()), ("solo", "solo"));
        assert!(solo.resolve(Some("solo")).is_ok());
        assert!(solo.resolve(Some("beta")).is_err());

        let shared = Caller::Token(m.authenticate("shared-token").unwrap().clone());
        assert!(
            shared
                .resolve(None)
                .unwrap_err()
                .contains("`as` is required")
        );
        let id = shared.resolve(Some("beta")).unwrap();
        assert_eq!((id.actor.as_str(), id.claim.as_str()), ("beta", "cloud"));
        assert!(shared.resolve(Some("solo")).is_err());

        let access = Caller::Access(Identity {
            actor: "a".into(),
            claim: "node-a".into(),
        });
        assert_eq!(access.resolve(None).unwrap().claim, "node-a");
        assert!(access.resolve(Some("b")).is_err());
    }

    #[test]
    fn directory_joins_both_bindings() {
        let ids = IdentityMap::parse(r#"{"cid":"node-a"}"#).unwrap();
        assert_eq!(
            directory(Some(&ids), Some(&map())),
            ["alpha", "beta", "node-a", "solo"]
        );
        assert!(directory(None, None).is_empty());
    }
}
