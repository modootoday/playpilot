//! `DeliveryStore` on Durable Object SQLite: `deliveries` (one row per
//! webhook and event) and `disabled_targets` (webhooks that answered 410).

use serde::Deserialize;
use worker::{SqlStorage, SqlStorageValue};

use crate::domain::outbound::store::DeliveryStore;
use crate::domain::outbound::{Delivery, State};

const SCHEMA: [&str; 3] = [
    "CREATE TABLE IF NOT EXISTS deliveries (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        id TEXT NOT NULL, webhook TEXT NOT NULL, event_id TEXT NOT NULL,
        ty TEXT NOT NULL, body TEXT NOT NULL, created_at INTEGER NOT NULL,
        attempts INTEGER NOT NULL, next_at INTEGER NOT NULL, state TEXT NOT NULL,
        last_status INTEGER, detail TEXT, updated_at INTEGER NOT NULL,
        UNIQUE (webhook, event_id))",
    "CREATE INDEX IF NOT EXISTS deliveries_due ON deliveries (state, next_at)",
    "CREATE TABLE IF NOT EXISTS disabled_targets (
        webhook TEXT NOT NULL, url TEXT NOT NULL, at INTEGER NOT NULL,
        PRIMARY KEY (webhook, url))",
];

const COLUMNS: &str = "seq, id, webhook, event_id, ty, body, created_at, attempts, next_at, state, last_status, detail, updated_at";

/// SQLite numbers reach Rust as JavaScript numbers.
#[derive(Deserialize)]
struct Row {
    seq: f64,
    id: String,
    webhook: String,
    event_id: String,
    ty: String,
    body: String,
    created_at: f64,
    attempts: f64,
    next_at: f64,
    state: State,
    last_status: Option<f64>,
    detail: Option<String>,
    updated_at: f64,
}

impl From<Row> for Delivery {
    fn from(r: Row) -> Self {
        Delivery {
            seq: r.seq as u64,
            id: r.id,
            webhook: r.webhook,
            event_id: r.event_id,
            ty: r.ty,
            body: r.body,
            created_at: r.created_at as u64,
            attempts: r.attempts as u32,
            next_at: r.next_at as u64,
            state: r.state,
            last_status: r.last_status.map(|s| s as u16),
            detail: r.detail,
            updated_at: r.updated_at as u64,
        }
    }
}

#[derive(Deserialize)]
struct At {
    at: Option<f64>,
}

pub struct SqlDeliveries {
    sql: SqlStorage,
}

fn s(e: worker::Error) -> String {
    e.to_string()
}

fn int(n: u64) -> SqlStorageValue {
    SqlStorageValue::Integer(n as i64)
}

fn state(st: State) -> SqlStorageValue {
    match st {
        State::Pending => "pending",
        State::Delivered => "delivered",
        State::Failed => "failed",
    }
    .into()
}

fn opt_text(v: &Option<String>) -> SqlStorageValue {
    v.clone()
        .map_or(SqlStorageValue::Null, SqlStorageValue::from)
}

impl SqlDeliveries {
    pub fn open(sql: SqlStorage) -> worker::Result<Self> {
        for stmt in SCHEMA {
            sql.exec(stmt, None)?;
        }
        Ok(Self { sql })
    }

    fn rows(&self, query: &str, args: Vec<SqlStorageValue>) -> Result<Vec<Delivery>, String> {
        let rows: Vec<Row> = self
            .sql
            .exec(query, args)
            .and_then(|c| c.to_array())
            .map_err(s)?;
        Ok(rows.into_iter().map(Delivery::from).collect())
    }
}

impl DeliveryStore for SqlDeliveries {
    fn enqueue(&self, d: &Delivery) -> Result<bool, String> {
        let cursor = self
            .sql
            .exec(
                "INSERT INTO deliveries (id, webhook, event_id, ty, body, created_at, attempts,
                    next_at, state, last_status, detail, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?)
                 ON CONFLICT(webhook, event_id) DO NOTHING",
                vec![
                    d.id.as_str().into(),
                    d.webhook.as_str().into(),
                    d.event_id.as_str().into(),
                    d.ty.as_str().into(),
                    d.body.as_str().into(),
                    int(d.created_at),
                    int(u64::from(d.attempts)),
                    int(d.next_at),
                    state(d.state),
                    int(d.updated_at),
                ],
            )
            .map_err(s)?;
        Ok(cursor.rows_written() > 0)
    }

    fn save(&self, d: &Delivery) -> Result<(), String> {
        let status = d
            .last_status
            .map_or(SqlStorageValue::Null, |v| int(u64::from(v)));
        self.sql
            .exec(
                "UPDATE deliveries SET attempts = ?, next_at = ?, state = ?, last_status = ?,
                    detail = ?, updated_at = ? WHERE seq = ?",
                vec![
                    int(u64::from(d.attempts)),
                    int(d.next_at),
                    state(d.state),
                    status,
                    opt_text(&d.detail),
                    int(d.updated_at),
                    int(d.seq),
                ],
            )
            .map(|_| ())
            .map_err(s)
    }

    fn due(&self, now: u64, limit: usize) -> Result<Vec<Delivery>, String> {
        let q = format!(
            "SELECT {COLUMNS} FROM deliveries WHERE state = 'pending' AND next_at <= ?
             ORDER BY next_at, seq LIMIT ?"
        );
        self.rows(&q, vec![int(now), int(limit as u64)])
    }

    fn next_due(&self) -> Result<Option<u64>, String> {
        let rows: Vec<At> = self
            .sql
            .exec(
                "SELECT MIN(next_at) AS at FROM deliveries WHERE state = 'pending'",
                None,
            )
            .and_then(|c| c.to_array())
            .map_err(s)?;
        Ok(rows.first().and_then(|r| r.at).map(|a| a as u64))
    }

    fn after(
        &self,
        after: u64,
        limit: usize,
        webhook: Option<&str>,
    ) -> Result<Vec<Delivery>, String> {
        match webhook {
            None => {
                let q =
                    format!("SELECT {COLUMNS} FROM deliveries WHERE seq > ? ORDER BY seq LIMIT ?");
                self.rows(&q, vec![int(after), int(limit as u64)])
            }
            Some(w) => {
                let q = format!(
                    "SELECT {COLUMNS} FROM deliveries WHERE seq > ? AND webhook = ?
                     ORDER BY seq LIMIT ?"
                );
                self.rows(&q, vec![int(after), w.into(), int(limit as u64)])
            }
        }
    }

    fn prune(&self, before: u64, keep: usize) -> Result<(), String> {
        self.sql
            .exec(
                "DELETE FROM deliveries WHERE state != 'pending' AND updated_at < ?",
                vec![int(before)],
            )
            .map_err(s)?;
        self.sql
            .exec(
                "DELETE FROM deliveries WHERE state != 'pending' AND seq NOT IN
                 (SELECT seq FROM deliveries WHERE state != 'pending' ORDER BY seq DESC LIMIT ?)",
                vec![int(keep as u64)],
            )
            .map(|_| ())
            .map_err(s)
    }

    fn disabled(&self, webhook: &str, url: &str) -> Result<bool, String> {
        let rows: Vec<serde_json::Value> = self
            .sql
            .exec(
                "SELECT 1 AS hit FROM disabled_targets WHERE webhook = ? AND url = ?",
                vec![webhook.into(), url.into()],
            )
            .and_then(|c| c.to_array())
            .map_err(s)?;
        Ok(!rows.is_empty())
    }

    fn disable(&self, webhook: &str, url: &str, at: u64) -> Result<(), String> {
        self.sql
            .exec(
                "INSERT INTO disabled_targets (webhook, url, at) VALUES (?, ?, ?)
                 ON CONFLICT(webhook, url) DO NOTHING",
                vec![webhook.into(), url.into(), int(at)],
            )
            .map(|_| ())
            .map_err(s)
    }
}
