//! Caller identity from the Cloudflare Access service token.
//!
//! Access sits in front of the Worker, checks the token pair and forwards the
//! request with `Cf-Access-Jwt-Assertion`. The Worker requires that header so
//! a route without Access fails closed, then maps the client id through
//! `PLAYPILOT_IDENTITIES`. Verifying the JWT signature is not done yet.

use worker::{Env, Request, Response, Result};

use super::{http, settings};
use crate::domain::identity::Identity;

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
