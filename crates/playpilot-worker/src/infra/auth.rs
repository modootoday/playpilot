//! Caller identity.
//!
//! `/v1/*`: Access sits in front of the Worker, checks the service token pair
//! and forwards the request with `Cf-Access-Jwt-Assertion`. The Worker
//! requires that header so a route without Access fails closed, then maps the
//! client id through `PLAYPILOT_IDENTITIES`. Verifying the JWT signature is
//! not done yet.
//!
//! `/mcp`: served on a host without Access. The Worker hashes the
//! `Authorization: Bearer` token and looks it up in `PLAYPILOT_MCP_TOKENS`;
//! Access headers are ignored there.

use worker::{Env, Request, Response, Result};

use super::{http, settings};
use crate::domain::identity::Identity;
use crate::domain::token::{Grant, bearer};

const CLIENT_ID: &str = "Cf-Access-Client-Id";
const ASSERTION: &str = "Cf-Access-Jwt-Assertion";

pub fn identify(req: &Request, env: &Env) -> std::result::Result<Identity, Result<Response>> {
    let header = |name: &str| {
        req.headers()
            .get(name)
            .ok()
            .flatten()
            .filter(|v| !v.is_empty())
    };
    let deny = || {
        Err(http::problem(
            401,
            "Unauthorized",
            "missing or unknown service token",
        ))
    };
    if header(ASSERTION).is_none() {
        return deny();
    }
    let Some(client) = header(CLIENT_ID) else {
        return deny();
    };
    let map = match settings::identities(env) {
        Ok(m) => m,
        Err(e) => return Err(http::internal(e)),
    };
    match map.lookup(&client) {
        Some(id) => Ok(id.clone()),
        None => deny(),
    }
}

/// A `/mcp` caller's grant. Any failure is a bare 401: the response does not
/// say whether the header, the token or the binding was wrong.
pub fn bearer_grant(req: &Request, env: &Env) -> std::result::Result<Grant, Result<Response>> {
    let deny = || {
        let resp = http::empty(401).and_then(|mut r| {
            r.headers_mut().set("WWW-Authenticate", "Bearer")?;
            Ok(r)
        });
        Err(resp)
    };
    let header = req.headers().get("Authorization").ok().flatten();
    let Some(token) = bearer(header.as_deref()) else {
        return deny();
    };
    let map = match settings::tokens(env) {
        Ok(m) => m,
        Err(e) => {
            worker::console_error!("mcp auth: {e}");
            return deny();
        }
    };
    match map.authenticate(token) {
        Some(g) => Ok(g.clone()),
        None => deny(),
    }
}
