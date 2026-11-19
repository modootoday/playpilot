//! Access assertion verification: team keys fetched from the certs endpoint
//! and cached per isolate, RS256 checked with WebCrypto, claims checked by
//! `crate::domain::access`.

use std::cell::RefCell;

use serde_json::json;
use worker::js_sys::{self, Array, JSON, Object, Promise};
use worker::wasm_bindgen::{JsCast, JsValue};
use worker::wasm_bindgen_futures::JsFuture;
use worker::web_sys::{CryptoKey, SubtleCrypto, WorkerGlobalScope};
use worker::{Fetch, Url};

use crate::domain::access::Reject;
use crate::domain::access::jwks::{Jwk, Jwks, KeyCache, Lookup};
use crate::domain::access::jwt::{Claims, parse};
use crate::domain::access::policy::Policy;

thread_local! {
    static KEYS: RefCell<KeyCache> = RefCell::new(KeyCache::default());
}

/// Why a request did not get verified claims.
pub enum Failure {
    /// The token is not acceptable.
    Rejected(Reject),
    /// The keys could not be fetched or the crypto call failed.
    Unavailable(String),
}

/// Checks `assertion` against `policy` at `now` and returns its claims.
pub async fn verify(assertion: &str, policy: &Policy, now: u64) -> Result<Claims, Failure> {
    let token = parse(assertion).map_err(Failure::Rejected)?;
    let key = key(policy, &token.kid, now).await?;
    let ok = rs256(&key, token.signing_input.as_bytes(), &token.signature)
        .await
        .map_err(Failure::Unavailable)?;
    if !ok {
        return Err(Failure::Rejected(Reject::Signature));
    }
    policy
        .check(&token.claims, now)
        .map_err(Failure::Rejected)?;
    Ok(token.claims)
}

async fn key(policy: &Policy, kid: &str, now: u64) -> Result<Jwk, Failure> {
    let url = policy.certs_url();
    let found = KEYS.with(|c| c.borrow().lookup(&url, kid, now));
    let found = match found {
        Lookup::Fetch => {
            let keys = fetch_keys(&url).await.map_err(Failure::Unavailable)?;
            KEYS.with(|c| {
                let mut cache = c.borrow_mut();
                cache.store(&url, now, keys);
                cache.after_fetch(&url, kid, now)
            })
        }
        other => other,
    };
    match found {
        Lookup::Hit(k) => Ok(k),
        Lookup::Fetch | Lookup::Unknown => Err(Failure::Rejected(Reject::UnknownKey)),
    }
}

async fn fetch_keys(url: &str) -> Result<Jwks, String> {
    let url = Url::parse(url).map_err(|e| format!("certs url: {e}"))?;
    let mut resp = Fetch::Url(url)
        .send()
        .await
        .map_err(|e| format!("certs fetch: {e}"))?;
    if resp.status_code() != 200 {
        return Err(format!("certs fetch: status {}", resp.status_code()));
    }
    Jwks::parse(&resp.text().await.map_err(|e| format!("certs body: {e}"))?)
}

fn js_object(v: &serde_json::Value) -> Result<Object, String> {
    JSON::parse(&v.to_string())
        .map(JsCast::unchecked_into)
        .map_err(|_| "could not build a JS object".to_string())
}

async fn settle(p: Result<Promise, JsValue>) -> Result<JsValue, String> {
    let p = p.map_err(|e| format!("webcrypto: {e:?}"))?;
    JsFuture::from(p)
        .await
        .map_err(|e| format!("webcrypto: {e:?}"))
}

fn subtle() -> Result<SubtleCrypto, String> {
    let scope: WorkerGlobalScope = js_sys::global().unchecked_into();
    scope
        .crypto()
        .map(|c| c.subtle())
        .map_err(|e| format!("webcrypto: {e:?}"))
}

/// RSASSA-PKCS1-v1_5 with SHA-256 over `data`.
async fn rs256(key: &Jwk, data: &[u8], signature: &[u8]) -> Result<bool, String> {
    let subtle = subtle()?;
    let jwk =
        js_object(&json!({"kty": "RSA", "n": key.n, "e": key.e, "alg": "RS256", "ext": true}))?;
    let algorithm = js_object(&json!({"name": "RSASSA-PKCS1-v1_5", "hash": {"name": "SHA-256"}}))?;
    let usages = Array::of1(&JsValue::from_str("verify"));
    let imported =
        settle(subtle.import_key_with_object("jwk", &jwk, &algorithm, false, &usages)).await?;
    let crypto_key: CryptoKey = imported.unchecked_into();
    let verified = settle(subtle.verify_with_object_and_u8_array_and_u8_array(
        &algorithm,
        &crypto_key,
        signature,
        data,
    ))
    .await?;
    verified
        .as_bool()
        .ok_or_else(|| "webcrypto: verify did not return a boolean".to_string())
}
