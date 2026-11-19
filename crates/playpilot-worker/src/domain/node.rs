//! Node heartbeats: last seen, running rev, lanes and runtime capacity.
//! A node with no heartbeat for `offline_after` seconds is offline.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ids::{valid_lane, valid_name, valid_rev};

pub const DEFAULT_OFFLINE_AFTER: u64 = 10 * 60;
const MAX_LIST: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy: Option<u64>,
}

/// The OpenAPI `Heartbeat` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Heartbeat {
    pub rev: String,
    #[serde(default)]
    pub lanes: Vec<String>,
    #[serde(default)]
    pub runtimes: Vec<Runtime>,
    #[serde(default)]
    pub leases: Vec<String>,
}

impl Heartbeat {
    pub fn parse(body: &str) -> Result<Self, String> {
        let hb: Self = serde_json::from_str(body).map_err(|e| format!("heartbeat: {e}"))?;
        if !valid_rev(&hb.rev) {
            return Err("rev must be a full 40-character commit SHA".into());
        }
        if hb.lanes.len() > MAX_LIST || hb.runtimes.len() > MAX_LIST || hb.leases.len() > MAX_LIST {
            return Err(format!("lists are limited to {MAX_LIST} entries"));
        }
        if let Some(bad) = hb.lanes.iter().find(|l| !valid_lane(l)) {
            return Err(format!("`{bad}` is not a lane name"));
        }
        if hb
            .runtimes
            .iter()
            .any(|r| r.kind.is_empty() || r.kind.len() > 64)
        {
            return Err("runtime type must be 1 to 64 characters".into());
        }
        Ok(hb)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRecord {
    pub rev: String,
    pub last_seen: u64,
    pub lanes: Vec<String>,
    pub runtimes: Vec<Runtime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Online,
    Offline,
    Paused,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeBook {
    pub nodes: BTreeMap<String, NodeRecord>,
}

impl NodeBook {
    pub fn heartbeat(&mut self, id: &str, hb: &Heartbeat, now: u64) -> Result<(), String> {
        if !valid_name(id) {
            return Err(format!("`{id}` is not a node id"));
        }
        self.nodes.insert(
            id.to_string(),
            NodeRecord {
                rev: hb.rev.clone(),
                last_seen: now,
                lanes: hb.lanes.clone(),
                runtimes: hb.runtimes.clone(),
            },
        );
        Ok(())
    }

    /// Paused (by effective ops) beats online; silence beats both.
    pub fn status(&self, id: &str, now: u64, offline_after: u64, paused: bool) -> Status {
        match self.nodes.get(id) {
            Some(n) if now.saturating_sub(n.last_seen) < offline_after => {
                if paused {
                    Status::Paused
                } else {
                    Status::Online
                }
            }
            _ => Status::Offline,
        }
    }

    /// Nodes that went silent: last seen at least `offline_after` ago.
    pub fn offline(&self, now: u64, offline_after: u64) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|(_, n)| now.saturating_sub(n.last_seen) >= offline_after)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeCmd {
    Heartbeat { id: String, heartbeat: Heartbeat },
    List,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeReply {
    Ok,
    Nodes(BTreeMap<String, NodeRecord>),
    Error(String),
}

pub fn apply(book: &mut NodeBook, cmd: NodeCmd, now: u64) -> NodeReply {
    match cmd {
        NodeCmd::Heartbeat { id, heartbeat } => match book.heartbeat(&id, &heartbeat, now) {
            Ok(()) => NodeReply::Ok,
            Err(e) => NodeReply::Error(e),
        },
        NodeCmd::List => NodeReply::Nodes(book.nodes.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hb() -> Heartbeat {
        Heartbeat::parse(&format!(
            r#"{{"rev":"{}","lanes":["build"]}}"#,
            "a".repeat(40)
        ))
        .unwrap()
    }

    #[test]
    fn parses_and_refuses() {
        let rev = "b".repeat(40);
        assert!(Heartbeat::parse(&format!(r#"{{"rev":"{rev}","extra":1}}"#)).is_err());
        assert!(Heartbeat::parse(r#"{"rev":"abc"}"#).is_err());
        assert!(Heartbeat::parse(&format!(r#"{{"rev":"{rev}","lanes":["Bad Lane"]}}"#)).is_err());
        let full = format!(
            r#"{{"rev":"{rev}","runtimes":[{{"type":"wsl","capacity":2,"busy":1}}],"leases":["l1"]}}"#
        );
        let parsed = Heartbeat::parse(&full).unwrap();
        assert_eq!(parsed.runtimes[0].busy, Some(1));
        assert_eq!(parsed.leases, vec!["l1"]);
    }

    #[test]
    fn online_then_offline() {
        let mut b = NodeBook::default();
        b.heartbeat("node-a", &hb(), 1_000).unwrap();
        assert_eq!(b.status("node-a", 1_100, 600, false), Status::Online);
        assert_eq!(b.status("node-a", 1_100, 600, true), Status::Paused);
        assert_eq!(b.status("node-a", 1_600, 600, false), Status::Offline);
        assert_eq!(b.status("node-z", 1_100, 600, false), Status::Offline);
        assert_eq!(b.offline(1_600, 600), vec!["node-a"]);
        b.heartbeat("node-a", &hb(), 1_590).unwrap();
        assert!(b.offline(1_600, 600).is_empty());
    }

    #[test]
    fn refuses_bad_ids() {
        let mut b = NodeBook::default();
        assert!(b.heartbeat("Desk Top", &hb(), 0).is_err());
        assert_eq!(
            apply(
                &mut b,
                NodeCmd::Heartbeat {
                    id: "n".into(),
                    heartbeat: hb()
                },
                5
            ),
            NodeReply::Ok
        );
        match apply(&mut b, NodeCmd::List, 6) {
            NodeReply::Nodes(n) => assert_eq!(n["n"].last_seen, 5),
            other => panic!("{other:?}"),
        }
    }
}
