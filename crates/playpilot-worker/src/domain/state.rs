//! `GET /v1/state`: the public subset. Node ids (opaque), statuses and lane
//! counts only; no items, repositories or device names.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::node::{NodeRecord, Status};
use super::ops::Switches;
use super::time::rfc3339;

pub struct Inputs<'a> {
    pub nodes: &'a BTreeMap<String, NodeRecord>,
    pub leased: &'a BTreeMap<String, u64>,
    pub config_lanes: &'a [String],
    pub switches: &'a Switches,
    pub now: u64,
    pub offline_after: u64,
}

pub fn build(i: &Inputs) -> Value {
    let nodes: Vec<Value> = i
        .nodes
        .iter()
        .map(|(id, n)| {
            let paused = i.switches.effective("Device", id).is_paused();
            let fresh = i.now.saturating_sub(n.last_seen) < i.offline_after;
            let status = match (fresh, paused) {
                (false, _) => Status::Offline,
                (true, true) => Status::Paused,
                (true, false) => Status::Online,
            };
            json!({"id": id, "status": status, "lastSeen": rfc3339(n.last_seen)})
        })
        .collect();
    let names: BTreeSet<&String> = i
        .config_lanes
        .iter()
        .chain(i.leased.keys())
        .chain(i.nodes.values().flat_map(|n| n.lanes.iter()))
        .collect();
    let lanes: Vec<Value> = names
        .into_iter()
        .map(|name| {
            json!({
                "name": name,
                "paused": i.switches.effective("Lane", name).is_paused(),
                // The queue lives on GitHub; the Worker does not read it yet.
                "queued": 0,
                "leased": i.leased.get(name).copied().unwrap_or(0),
            })
        })
        .collect();
    json!({"generatedAt": rfc3339(i.now), "nodes": nodes, "lanes": lanes})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ops::OpsTable;

    fn rec(last_seen: u64, lanes: &[&str]) -> NodeRecord {
        NodeRecord {
            rev: "a".repeat(40),
            last_seen,
            lanes: lanes.iter().map(|s| s.to_string()).collect(),
            runtimes: vec![],
        }
    }

    #[test]
    fn counts_only() {
        let nodes = BTreeMap::from([
            ("node-a".to_string(), rec(1_000, &["build"])),
            ("node-b".to_string(), rec(0, &["docs"])),
            ("node-c".to_string(), rec(1_000, &[])),
        ]);
        let leased = BTreeMap::from([("build".to_string(), 2)]);
        let switches = Switches {
            file: OpsTable::default(),
            worker: OpsTable::parse(
                r#"{"Device/node-c":{"paused":true},"Lane/docs":{"paused":true}}"#,
            )
            .unwrap(),
        };
        let v = build(&Inputs {
            nodes: &nodes,
            leased: &leased,
            config_lanes: &["ops".to_string()],
            switches: &switches,
            now: 1_100,
            offline_after: 600,
        });
        let status: Vec<&str> = v["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["status"].as_str().unwrap())
            .collect();
        assert_eq!(status, vec!["online", "offline", "paused"]);
        let lanes = v["lanes"].as_array().unwrap();
        let names: Vec<&str> = lanes.iter().map(|l| l["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["build", "docs", "ops"]);
        assert_eq!(lanes[0]["leased"], 2);
        assert_eq!(lanes[1]["paused"], true);
        assert!(!v.to_string().contains("rev"));
    }

    /// Node ids are the Access claim ids; device names, revs, runtimes and
    /// node lanes never reach the public subset.
    #[test]
    fn only_public_fields() {
        let mut laptop = rec(1_000, &["build"]);
        laptop.runtimes = vec![super::super::node::Runtime {
            kind: "claude-code".into(),
            capacity: Some(2),
            busy: Some(1),
        }];
        let nodes = BTreeMap::from([("node-a".to_string(), laptop)]);
        let v = build(&Inputs {
            nodes: &nodes,
            leased: &BTreeMap::new(),
            config_lanes: &[],
            switches: &Switches {
                file: OpsTable::default(),
                worker: OpsTable::default(),
            },
            now: 1_100,
            offline_after: 600,
        });
        let keys =
            |v: &Value| -> BTreeSet<String> { v.as_object().unwrap().keys().cloned().collect() };
        let set = |k: &[&str]| -> BTreeSet<String> { k.iter().map(|s| s.to_string()).collect() };
        assert_eq!(keys(&v), set(&["generatedAt", "lanes", "nodes"]));
        assert_eq!(keys(&v["nodes"][0]), set(&["id", "lastSeen", "status"]));
        assert_eq!(
            keys(&v["lanes"][0]),
            set(&["leased", "name", "paused", "queued"])
        );
        assert!(!v.to_string().contains("claude-code"));
    }
}
