//! Workers runtime bindings: the fetch entry point, the router and the three
//! Durable Object classes. Logic lives in `crate::domain`.

mod api;
mod auth;
mod call;
mod http;
mod mcp;
mod objects;
mod settings;
mod wake;

use worker::{Context, Env, Request, Response, Result, event};

use crate::domain::route::{Auth, Route, route};
use crate::domain::token::Caller;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let route = route(req.method().as_ref(), &req.path());
    if route == Route::Mcp {
        return match auth::bearer_grant(&req, &env) {
            Ok(grant) => mcp::handle(req, &env, &Caller::Token(grant)).await,
            Err(resp) => resp,
        };
    }
    let caller = if route.auth() == Auth::Access {
        match auth::identify(&req, &env) {
            Ok(id) => Some(id),
            Err(resp) => return resp,
        }
    } else {
        None
    };
    match (route, caller) {
        (Route::State, _) => api::state(&env).await,
        (Route::NotFound, _) => http::problem(404, "Not found", "no such endpoint"),
        (Route::MethodNotAllowed(allow), _) => http::method_not_allowed(allow),
        (Route::Config, Some(_)) => api::config(&req, &env),
        (Route::CreateLease, Some(id)) => api::create_lease(req, &env, &id).await,
        (Route::ReleaseLease(lease), Some(id)) => api::release_lease(&env, &id, &lease).await,
        (Route::Heartbeat(node), Some(id)) => api::heartbeat(req, &env, &id, &node).await,
        (Route::Events, Some(_)) => api::events(req, &env).await,
        (Route::Mcp, _) => http::problem(401, "Unauthorized", "a bearer token is required"),
        (_, None) => http::problem(401, "Unauthorized", "a service token is required"),
    }
}
