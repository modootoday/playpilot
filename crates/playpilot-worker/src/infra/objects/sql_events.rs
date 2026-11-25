//! `EventStore` on Durable Object SQLite: `log` (events in order) and
//! `seen` (delivery ids, kept by age only).

use serde::Deserialize;
use worker::{SqlStorage, SqlStorageValue};

use crate::domain::eventlog::Entry;
use crate::domain::eventlog::store::EventStore;

const SCHEMA: [&str; 3] = [
    "CREATE TABLE IF NOT EXISTS log (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        received_at INTEGER NOT NULL, ty TEXT NOT NULL, json TEXT NOT NULL)",
    "CREATE TABLE IF NOT EXISTS seen (id TEXT PRIMARY KEY, received_at INTEGER NOT NULL)",
    "CREATE INDEX IF NOT EXISTS seen_age ON seen (received_at)",
];

/// SQLite numbers reach Rust as JavaScript numbers.
#[derive(Deserialize)]
struct Row {
    seq: f64,
    received_at: f64,
    ty: String,
    json: String,
}

#[derive(Deserialize)]
struct Seq {
    seq: f64,
}

pub struct SqlEvents {
    sql: SqlStorage,
}

fn s(e: worker::Error) -> String {
    e.to_string()
}

fn int(n: u64) -> SqlStorageValue {
    SqlStorageValue::Integer(n as i64)
}

impl SqlEvents {
    pub fn open(sql: SqlStorage) -> worker::Result<Self> {
        for stmt in SCHEMA {
            sql.exec(stmt, None)?;
        }
        Ok(Self { sql })
    }
}

impl EventStore for SqlEvents {
    fn seen(&self, id: &str) -> Result<bool, String> {
        let rows: Vec<serde_json::Value> = self
            .sql
            .exec("SELECT 1 AS hit FROM seen WHERE id = ?", vec![id.into()])
            .and_then(|c| c.to_array())
            .map_err(s)?;
        Ok(!rows.is_empty())
    }

    fn insert(&self, id: &str, received_at: u64, ty: &str, json: &str) -> Result<u64, String> {
        self.sql
            .exec(
                "INSERT INTO seen (id, received_at) VALUES (?, ?) ON CONFLICT(id) DO NOTHING",
                vec![id.into(), int(received_at)],
            )
            .map_err(s)?;
        let rows: Vec<Seq> = self
            .sql
            .exec(
                "INSERT INTO log (received_at, ty, json) VALUES (?, ?, ?) RETURNING seq",
                vec![int(received_at), ty.into(), json.into()],
            )
            .and_then(|c| c.to_array())
            .map_err(s)?;
        rows.first()
            .map(|r| r.seq as u64)
            .ok_or_else(|| "insert returned no seq".into())
    }

    fn prune(&self, before: u64, keep: usize) -> Result<(), String> {
        self.sql
            .exec("DELETE FROM log WHERE received_at < ?", vec![int(before)])
            .map_err(s)?;
        self.sql
            .exec(
                "DELETE FROM log WHERE seq NOT IN (SELECT seq FROM log ORDER BY seq DESC LIMIT ?)",
                vec![int(keep as u64)],
            )
            .map(|_| ())
            .map_err(s)
    }

    fn forget_seen(&self, before: u64) -> Result<(), String> {
        self.sql
            .exec("DELETE FROM seen WHERE received_at < ?", vec![int(before)])
            .map(|_| ())
            .map_err(s)
    }

    fn after(
        &self,
        after: u64,
        limit: usize,
        ty_prefix: Option<&str>,
    ) -> Result<Vec<Entry>, String> {
        let rows: Vec<Row> = match ty_prefix {
            None => self.sql.exec(
                "SELECT seq, received_at, ty, json FROM log WHERE seq > ? ORDER BY seq LIMIT ?",
                vec![int(after), int(limit as u64)],
            ),
            Some(p) => {
                let dotted = format!("{p}.");
                self.sql.exec(
                    "SELECT seq, received_at, ty, json FROM log
                     WHERE seq > ? AND (ty = ? OR substr(ty, 1, ?) = ?)
                     ORDER BY seq LIMIT ?",
                    vec![
                        int(after),
                        p.into(),
                        int(dotted.len() as u64),
                        dotted.as_str().into(),
                        int(limit as u64),
                    ],
                )
            }
        }
        .and_then(|c| c.to_array())
        .map_err(s)?;
        Ok(rows
            .into_iter()
            .map(|r| Entry {
                seq: r.seq as u64,
                received_at: r.received_at as u64,
                ty: r.ty,
                json: r.json,
            })
            .collect())
    }
}
