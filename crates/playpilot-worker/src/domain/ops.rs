//! Narrow-only runtime switches. The file (Git) and the Worker each set `ops`
//! per `Kind/name`; the effective value is always the narrower of the two.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ops {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<u64>,
}

impl Ops {
    /// Paused if either side pauses; the lower capacity if either sets one.
    pub fn narrower(self, other: Ops) -> Ops {
        let paused = match (self.paused, other.paused) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(false) || b.unwrap_or(false)),
        };
        let capacity = match (self.capacity, other.capacity) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        Ops { paused, capacity }
    }

    pub fn is_paused(&self) -> bool {
        self.paused == Some(true)
    }

    /// Reads `{paused, capacity}`; unknown keys are ignored, wrong types refused.
    pub fn from_value(v: &Value) -> Result<Ops, String> {
        let obj = v.as_object().ok_or("ops must be an object")?;
        let paused = match obj.get("paused") {
            None | Some(Value::Null) => None,
            Some(p) => Some(p.as_bool().ok_or("ops.paused must be a boolean")?),
        };
        let capacity = match obj.get("capacity") {
            None | Some(Value::Null) => None,
            Some(c) => Some(c.as_u64().ok_or("ops.capacity must be an integer >= 0")?),
        };
        Ok(Ops { paused, capacity })
    }
}

/// `Kind/name` → ops, from one source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpsTable(pub BTreeMap<String, Ops>);

impl OpsTable {
    /// Parses a JSON object keyed by `Kind/name`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("ops: {e}"))?;
        let obj = v.as_object().ok_or("ops must be a JSON object")?;
        let mut out = BTreeMap::new();
        for (key, value) in obj {
            if !key.contains('/') {
                return Err(format!("ops key `{key}` is not Kind/name"));
            }
            out.insert(key.clone(), Ops::from_value(value)?);
        }
        Ok(Self(out))
    }

    pub fn get(&self, kind: &str, name: &str) -> Ops {
        self.0
            .get(&format!("{kind}/{name}"))
            .copied()
            .unwrap_or_default()
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(&self.0).unwrap_or(Value::Null)
    }
}

/// Effective ops: file ops narrowed by Worker ops.
#[derive(Debug, Clone, Default)]
pub struct Switches {
    pub file: OpsTable,
    pub worker: OpsTable,
}

impl Switches {
    pub fn effective(&self, kind: &str, name: &str) -> Ops {
        self.file
            .get(kind, name)
            .narrower(self.worker.get(kind, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrower_wins_both_ways() {
        let file = Ops {
            paused: Some(false),
            capacity: Some(3),
        };
        let worker = Ops {
            paused: Some(true),
            capacity: Some(5),
        };
        let eff = file.narrower(worker);
        assert_eq!(
            eff,
            Ops {
                paused: Some(true),
                capacity: Some(3)
            }
        );
        assert_eq!(worker.narrower(file), eff);
    }

    #[test]
    fn worker_cannot_widen() {
        let file = Ops {
            paused: Some(true),
            capacity: Some(1),
        };
        let worker = Ops {
            paused: Some(false),
            capacity: Some(10),
        };
        assert_eq!(file.narrower(worker), file);
        assert_eq!(Ops::default().narrower(Ops::default()), Ops::default());
    }

    #[test]
    fn parses_tables() {
        let t = OpsTable::parse(r#"{"Device/node-a":{"paused":true},"Lane/build":{"capacity":2}}"#)
            .unwrap();
        assert!(t.get("Device", "node-a").is_paused());
        assert_eq!(t.get("Lane", "build").capacity, Some(2));
        assert!(OpsTable::parse(r#"{"node-a":{}}"#).is_err());
        assert!(OpsTable::parse(r#"{"Lane/x":{"capacity":-1}}"#).is_err());
    }

    #[test]
    fn switches_combine() {
        let s = Switches {
            file: OpsTable::parse(r#"{"Lane/x":{"capacity":4}}"#).unwrap(),
            worker: OpsTable::parse(r#"{"Lane/x":{"capacity":1,"paused":false}}"#).unwrap(),
        };
        assert_eq!(s.effective("Lane", "x").capacity, Some(1));
        assert!(!s.effective("Lane", "x").is_paused());
    }
}
