//! Caller identity.
//!
//! `/v1/*`: Access sits in front of the Worker and forwards the request with
//! `Cf-Access-Jwt-Assertion`. The Worker verifies that JWT (team key
//! signature, issuer, audience, validity window) and maps its `common_name`,
//! the service token client id, through `PLAYPILOT_IDENTITIES`; every
//! failure is the same 401 (reason logged, not returned). Request
//! headers such as `Cf-Access-Client-Id` are never trusted. Without
//! `PLAYPILOT_ACCESS_TEAM_DOMAIN` and `PLAYPILOT_ACCESS_AUD` no caller passes.
//!
//! `/mcp`: served on a host without Access. The Worker hashes the
//! `Authorization: Bearer` token and looks it up in `PLAYPILOT_MCP_TOKENS`;
//! Access headers are ignored there.

use worker::{Env, Request, Response, Result};

use super::access::{Failure, verify};
use super::{http, settings};
use crate::domain::access::service_token_id;
use crate::domain::identity::Identity;
use crate::domain::token::{Grant, bearer};

const ASSERTION: &str = "Cf-Access-Jwt-Assertion";

fn unauthorized() -> std::result::Result<Identity, Result<Response>> {
    Err(http::problem(
        401,
        "Unauthorized",
        "missing or unknown service token",
    ))
}

pub async fn identify(req: &Request, env: &Env) -> std::result::Result<Identity, Result<Response>> {
    let policy = match settings::access_policy(env) {
        Ok(p) => p,
        Err(e) => {
            worker::console_error!("access: {e}");
            return unauthorized();
        }
    };
    let assertion = req.headers().get(ASSERTION).ok().flatten();
    let Some(assertion) = assertion.filter(|v| !v.is_empty()) else {
        return unauthorized();
    };
    let claims = match verify(&assertion, &policy, settings::now()).await {
        Ok(c) => c,
        Err(Failure::Rejected(why)) => {
            worker::console_warn!("access: {why}");
            return unauthorized();
        }
        Err(Failure::Unavailable(e)) => {
            worker::console_error!("access: {e}");
            return unauthorized();
        }
    };
    let Some(client) = service_token_id(&claims) else {
        return unauthorized();
    };
    let map = match settings::identities(env) {
        Ok(m) => m,
        Err(e) => return Err(http::internal(e)),
    };
    match map.lookup(client) {
        Some(id) => Ok(id.clone()),
        None => unauthorized(),
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
    if bearer(header.as_deref()).is_none() {
        return deny();
    }
    let map = match settings::tokens(env) {
        Ok(m) => m,
        Err(e) => {
            worker::console_error!("mcp auth: {e}");
            return deny();
        }
    };
    match map.grant_for(header.as_deref()) {
        Some(g) => Ok(g.clone()),
        None => deny(),
    }
}
