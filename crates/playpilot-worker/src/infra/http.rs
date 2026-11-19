//! Response helpers.

use serde_json::Value;
use worker::{Headers, Response, Result};

use crate::domain::problem;

pub fn json(status: u16, body: &Value) -> Result<Response> {
    Ok(Response::from_json(body)?.with_status(status))
}

fn typed(status: u16, body: String, content_type: &str) -> Result<Response> {
    let headers = Headers::new();
    headers.set("Content-Type", content_type)?;
    Ok(Response::ok(body)?
        .with_status(status)
        .with_headers(headers))
}

pub fn problem(status: u16, title: &str, detail: &str) -> Result<Response> {
    typed(
        status,
        problem::body(status, title, detail).to_string(),
        problem::CONTENT_TYPE,
    )
}

pub fn empty(status: u16) -> Result<Response> {
    Ok(Response::empty()?.with_status(status))
}

pub fn method_not_allowed(allow: &str) -> Result<Response> {
    let mut resp = problem(405, "Method not allowed", "see the Allow header")?;
    resp.headers_mut().set("Allow", allow)?;
    Ok(resp)
}

/// A server-side failure; the detail stays in the log, not the response.
pub fn internal(err: impl std::fmt::Display) -> Result<Response> {
    worker::console_error!("internal error: {err}");
    problem(
        500,
        "Internal error",
        "the Worker could not complete the request",
    )
}
