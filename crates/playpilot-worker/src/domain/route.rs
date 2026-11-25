//! Request routing for the public API, independent of the HTTP library.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Config,
    CreateLease,
    ReleaseLease(String),
    Heartbeat(String),
    Events,
    State,
    Mcp,
    /// `POST /hooks/github`: inbound GitHub webhooks.
    GithubHook,
    /// Known path, wrong method; carries the `Allow` header value.
    MethodNotAllowed(&'static str),
    NotFound,
}

/// How a route authenticates its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// Public: the state subset and errors.
    None,
    /// `/v1/*`: the Cloudflare Access service token.
    Access,
    /// `/mcp`: a bearer token the Worker checks itself, on any host.
    Bearer,
    /// `/hooks/*`: only the sender's payload signature; no Access, no bearer.
    Signature,
}

impl Route {
    pub fn auth(&self) -> Auth {
        match self {
            Route::State | Route::NotFound | Route::MethodNotAllowed(_) => Auth::None,
            Route::Mcp => Auth::Bearer,
            Route::GithubHook => Auth::Signature,
            _ => Auth::Access,
        }
    }
}

fn segment(s: &str) -> Option<String> {
    let ok = !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c));
    ok.then(|| s.to_string())
}

pub fn route(method: &str, path: &str) -> Route {
    let parts: Vec<&str> = path.trim_end_matches('/').split('/').skip(1).collect();
    let pick = |allow: &'static str, wanted: &str, ok: Route| {
        if method == wanted {
            ok
        } else {
            Route::MethodNotAllowed(allow)
        }
    };
    match parts.as_slice() {
        ["v1", "config"] => pick("GET", "GET", Route::Config),
        ["v1", "leases"] => pick("POST", "POST", Route::CreateLease),
        ["v1", "leases", id] => match segment(id) {
            Some(id) => pick("DELETE", "DELETE", Route::ReleaseLease(id)),
            None => Route::NotFound,
        },
        ["v1", "nodes", id, "heartbeat"] => match segment(id) {
            Some(id) => pick("POST", "POST", Route::Heartbeat(id)),
            None => Route::NotFound,
        },
        ["v1", "events"] => pick("POST", "POST", Route::Events),
        ["v1", "state"] => pick("GET", "GET", Route::State),
        ["mcp"] => pick("POST", "POST", Route::Mcp),
        ["hooks", "github"] => pick("POST", "POST", Route::GithubHook),
        _ => Route::NotFound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        assert_eq!(route("GET", "/v1/config"), Route::Config);
        assert_eq!(route("POST", "/v1/leases"), Route::CreateLease);
        assert_eq!(
            route("DELETE", "/v1/leases/l1-2"),
            Route::ReleaseLease("l1-2".into())
        );
        assert_eq!(
            route("POST", "/v1/nodes/node-a/heartbeat"),
            Route::Heartbeat("node-a".into())
        );
        assert_eq!(route("POST", "/v1/events"), Route::Events);
        assert_eq!(route("GET", "/v1/state/"), Route::State);
        assert_eq!(route("POST", "/mcp"), Route::Mcp);
        assert_eq!(route("POST", "/hooks/github"), Route::GithubHook);
    }

    #[test]
    fn rejects() {
        assert_eq!(route("POST", "/v1/config"), Route::MethodNotAllowed("GET"));
        assert_eq!(route("GET", "/mcp"), Route::MethodNotAllowed("POST"));
        assert_eq!(
            route("GET", "/hooks/github"),
            Route::MethodNotAllowed("POST")
        );
        assert_eq!(route("POST", "/hooks/other"), Route::NotFound);
        assert_eq!(route("GET", "/v1/other"), Route::NotFound);
        assert_eq!(route("DELETE", "/v1/leases/a%20b"), Route::NotFound);
        assert_eq!(route("GET", "/"), Route::NotFound);
    }

    #[test]
    fn identity_requirement() {
        assert_eq!(Route::State.auth(), Auth::None);
        assert_eq!(Route::NotFound.auth(), Auth::None);
        assert_eq!(Route::Config.auth(), Auth::Access);
        assert_eq!(Route::Heartbeat("n".into()).auth(), Auth::Access);
        assert_eq!(Route::Mcp.auth(), Auth::Bearer);
        assert_eq!(Route::GithubHook.auth(), Auth::Signature);
    }
}
