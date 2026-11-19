//! Fires an actor's routine after `mail_send`. Failures are logged and never
//! fail the send; the bus has already recorded the fire for its debounce.

use worker::wasm_bindgen::JsValue;
use worker::{Fetch, Headers, Method, Request, RequestInit};

use crate::domain::wake::{Due, WakeMap, fire_request};

pub async fn fire_all(map: &WakeMap, due: &[Due]) {
    for d in due {
        let Some(target) = map.get(&d.actor) else {
            continue;
        };
        let req = fire_request(target, &d.actor, d.unacked);
        match send(&req.url, &req.headers, &req.body).await {
            Ok((status, _)) if (200..300).contains(&status) => {}
            Ok((429, retry)) => worker::console_warn!(
                "wake {}: rate limited, retry after {}",
                d.actor,
                retry.as_deref().unwrap_or("?")
            ),
            Ok((status, _)) => worker::console_warn!("wake {}: status {status}", d.actor),
            Err(e) => worker::console_warn!("wake {}: {e}", d.actor),
        }
    }
}

async fn send(
    url: &str,
    headers: &[(&'static str, String)],
    body: &str,
) -> worker::Result<(u16, Option<String>)> {
    let h = Headers::new();
    for (name, value) in headers {
        h.set(name, value)?;
    }
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(h)
        .with_body(Some(JsValue::from_str(body)));
    let req = Request::new_with_init(url, &init)?;
    let resp = Fetch::Request(req).send().await?;
    let retry = resp.headers().get("Retry-After").ok().flatten();
    Ok((resp.status_code(), retry))
}
