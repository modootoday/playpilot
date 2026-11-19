//! Durable Object classes, each with SQLite storage (see `wrangler.toml`).

mod bus;
mod doc;
mod lease;
mod node;
mod sql_bus;

use worker::{Request, Response, Result};

/// Reads the JSON command a router call sent.
async fn command<C: serde::de::DeserializeOwned>(mut req: Request) -> Result<C> {
    serde_json::from_str(&req.text().await?).map_err(|e| worker::Error::RustError(e.to_string()))
}

fn reply<R: serde::Serialize>(r: &R) -> Result<Response> {
    Response::ok(serde_json::to_string(r).map_err(|e| worker::Error::RustError(e.to_string()))?)
}
