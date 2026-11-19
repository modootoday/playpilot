//! A single JSON document in a SQLite `doc` table, for the small, bounded
//! lease and node books (one row per object).

use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use worker::{Result, SqlStorage};

const KEY: &str = "book";

#[derive(Deserialize)]
struct Row {
    v: String,
}

fn err(e: impl std::fmt::Display) -> worker::Error {
    worker::Error::RustError(e.to_string())
}

pub fn init(sql: &SqlStorage) -> Result<()> {
    sql.exec(
        "CREATE TABLE IF NOT EXISTS doc (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        None,
    )?;
    Ok(())
}

pub fn load<T: DeserializeOwned + Default>(sql: &SqlStorage) -> Result<T> {
    let rows: Vec<Row> = sql
        .exec("SELECT v FROM doc WHERE k = ?", vec![KEY.into()])?
        .to_array()?;
    match rows.first() {
        Some(r) => serde_json::from_str(&r.v).map_err(err),
        None => Ok(T::default()),
    }
}

pub fn save<T: Serialize>(sql: &SqlStorage, value: &T) -> Result<()> {
    let v = serde_json::to_string(value).map_err(err)?;
    sql.exec(
        "INSERT INTO doc (k, v) VALUES (?, ?) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
        vec![KEY.into(), v.into()],
    )?;
    Ok(())
}
