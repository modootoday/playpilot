//! `BusStore` on Durable Object SQLite: `messages` and `events` tables.

use serde::Deserialize;
use worker::{SqlStorage, SqlStorageValue};

use crate::domain::bus::message::{Envelope, Receipt, Stored};
use crate::domain::bus::store::BusStore;

const SCHEMA: [&str; 3] = [
    "CREATE TABLE IF NOT EXISTS messages (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        mailbox TEXT NOT NULL, created_at INTEGER NOT NULL,
        from_actor TEXT NOT NULL, to_actor TEXT NOT NULL,
        subject TEXT NOT NULL, re TEXT, body TEXT NOT NULL,
        read INTEGER NOT NULL DEFAULT 0, acked INTEGER NOT NULL DEFAULT 0)",
    "CREATE INDEX IF NOT EXISTS messages_mailbox ON messages (mailbox, created_at)",
    "CREATE TABLE IF NOT EXISTS events (
        key TEXT PRIMARY KEY, received_at INTEGER NOT NULL, json TEXT NOT NULL)",
];

const COLUMNS: &str =
    "id, mailbox, created_at, from_actor, to_actor, subject, re, body, read, acked";

/// SQLite numbers reach Rust as JavaScript numbers.
#[derive(Deserialize)]
struct Row {
    id: f64,
    mailbox: String,
    created_at: f64,
    from_actor: String,
    to_actor: String,
    subject: String,
    re: Option<String>,
    body: String,
    read: f64,
    acked: f64,
}

impl From<Row> for Stored {
    fn from(r: Row) -> Self {
        Stored {
            id: r.id as u64,
            mailbox: r.mailbox,
            created_at: r.created_at as u64,
            envelope: Envelope {
                from: r.from_actor,
                to: r.to_actor,
                subject: r.subject,
                re: r.re,
                body: r.body,
            },
            read: r.read != 0.0,
            acked: r.acked != 0.0,
        }
    }
}

#[derive(Deserialize)]
struct Name {
    mailbox: String,
}

#[derive(Deserialize)]
struct Id {
    id: f64,
}

pub struct SqlBus {
    sql: SqlStorage,
}

fn s(e: worker::Error) -> String {
    e.to_string()
}

fn int(n: u64) -> SqlStorageValue {
    SqlStorageValue::Integer(n as i64)
}

impl SqlBus {
    pub fn open(sql: SqlStorage) -> worker::Result<Self> {
        for stmt in SCHEMA {
            sql.exec(stmt, None)?;
        }
        Ok(Self { sql })
    }

    fn rows(&self, query: &str, args: Vec<SqlStorageValue>) -> Result<Vec<Stored>, String> {
        let rows: Vec<Row> = self
            .sql
            .exec(query, args)
            .and_then(|c| c.to_array())
            .map_err(s)?;
        Ok(rows.into_iter().map(Stored::from).collect())
    }
}

impl BusStore for SqlBus {
    fn mailboxes(&self) -> Result<Vec<String>, String> {
        let rows: Vec<Name> = self
            .sql
            .exec(
                "SELECT DISTINCT mailbox FROM messages ORDER BY mailbox",
                None,
            )
            .and_then(|c| c.to_array())
            .map_err(s)?;
        Ok(rows.into_iter().map(|r| r.mailbox).collect())
    }

    fn insert(&self, mailbox: &str, created_at: u64, e: &Envelope) -> Result<u64, String> {
        let re =
            e.re.clone()
                .map_or(SqlStorageValue::Null, SqlStorageValue::from);
        let args = vec![
            mailbox.into(),
            int(created_at),
            e.from.as_str().into(),
            e.to.as_str().into(),
            e.subject.as_str().into(),
            re,
            e.body.as_str().into(),
        ];
        let ids: Vec<Id> = self
            .sql
            .exec(
                "INSERT INTO messages (mailbox, created_at, from_actor, to_actor, subject, re, body)
                 VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
                args,
            )
            .and_then(|c| c.to_array())
            .map_err(s)?;
        ids.first()
            .map(|r| r.id as u64)
            .ok_or_else(|| "insert returned no id".into())
    }

    fn list(&self, mailbox: &str) -> Result<Vec<Stored>, String> {
        let q = format!("SELECT {COLUMNS} FROM messages WHERE mailbox = ? ORDER BY created_at, id");
        self.rows(&q, vec![mailbox.into()])
    }

    fn get(&self, id: u64) -> Result<Option<Stored>, String> {
        let q = format!("SELECT {COLUMNS} FROM messages WHERE id = ?");
        Ok(self.rows(&q, vec![int(id)])?.into_iter().next())
    }

    fn mark(&self, id: u64, kind: Receipt) -> Result<(), String> {
        let q = match kind {
            Receipt::Read => "UPDATE messages SET read = 1 WHERE id = ?",
            Receipt::Ack => "UPDATE messages SET read = 1, acked = 1 WHERE id = ?",
        };
        self.sql.exec(q, vec![int(id)]).map(|_| ()).map_err(s)
    }

    fn record_event(&self, key: &str, received_at: u64, json: &str) -> Result<bool, String> {
        let cursor = self
            .sql
            .exec(
                "INSERT INTO events (key, received_at, json) VALUES (?, ?, ?) ON CONFLICT(key) DO NOTHING",
                vec![key.into(), int(received_at), json.into()],
            )
            .map_err(s)?;
        Ok(cursor.rows_written() > 0)
    }
}
