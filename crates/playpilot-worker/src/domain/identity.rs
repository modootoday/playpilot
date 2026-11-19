//! Caller identity: the Cloudflare Access service-token client id, mapped to
//! an actor (mailbox) and a claim id (lease holder, node id) by a binding.
//! Request bodies and tool arguments never choose who the caller is.

use std::collections::BTreeMap;

use serde_json::Value;

use super::ids::valid_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub actor: String,
    pub claim: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityMap(BTreeMap<String, Identity>);

impl IdentityMap {
    /// `{"<client id>": "node-a"}` or `{"<client id>": {"actor": "a", "claim": "node-a"}}`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("identities: {e}"))?;
        let obj = v.as_object().ok_or("identities must be a JSON object")?;
        let mut out = BTreeMap::new();
        for (client, entry) in obj {
            if client.trim().is_empty() {
                return Err("identities: empty client id".into());
            }
            let (actor, claim) = match entry {
                Value::String(name) => (name.as_str(), name.as_str()),
                Value::Object(o) => {
                    let get = |k: &str| o.get(k).and_then(Value::as_str);
                    let actor = get("actor").ok_or("identities: `actor` is required")?;
                    (actor, get("claim").unwrap_or(actor))
                }
                _ => return Err("identities: each value is a name or {actor, claim}".into()),
            };
            if !valid_name(actor) || !valid_name(claim) || actor == "all" {
                return Err(format!(
                    "identities: `{actor}`/`{claim}` is not a valid name"
                ));
            }
            let id = Identity {
                actor: actor.into(),
                claim: claim.into(),
            };
            out.insert(client.clone(), id);
        }
        Ok(Self(out))
    }

    pub fn lookup(&self, client_id: &str) -> Option<&Identity> {
        self.0.get(client_id)
    }

    /// Every configured actor, sorted and unique: the broadcast directory.
    pub fn actors(&self) -> Vec<String> {
        let mut a: Vec<String> = self.0.values().map(|i| i.actor.clone()).collect();
        a.sort();
        a.dedup();
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_client_ids() {
        let m = IdentityMap::parse(
            r#"{"cid-1.access":"node-a","cid-2.access":{"actor":"reviewer","claim":"node-b"}}"#,
        )
        .unwrap();
        assert_eq!(m.lookup("cid-1.access").unwrap().claim, "node-a");
        assert_eq!(m.lookup("cid-2.access").unwrap().actor, "reviewer");
        assert_eq!(m.lookup("cid-2.access").unwrap().claim, "node-b");
        assert!(m.lookup("unknown").is_none());
        assert_eq!(m.actors(), vec!["node-a", "reviewer"]);
    }

    #[test]
    fn refuses_bad_maps() {
        for bad in [
            r#"[]"#,
            r#"{"c":5}"#,
            r#"{"c":"Bad Name"}"#,
            r#"{"c":"all"}"#,
            r#"{"":"a"}"#,
            r#"{"c":{}}"#,
        ] {
            assert!(IdentityMap::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(IdentityMap::parse("{}").unwrap().actors().len(), 0);
    }
}
