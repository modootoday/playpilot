//! Standard Webhooks signing (<https://www.standardwebhooks.com/>): the
//! secret is `whsec_<base64>`, the signature `v1,<base64 HMAC-SHA256>` of
//! `{webhook-id}.{webhook-timestamp}.{body}` under the decoded secret bytes.

use sha2::{Digest, Sha256};

use crate::domain::hook::hmac::{constant_time_eq, hmac_sha256};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const PREFIX: &str = "whsec_";

/// Padded standard base64 (RFC 4648 section 4).
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes padded or unpadded standard base64; anything else is `None`.
pub fn unbase64(text: &str) -> Option<Vec<u8>> {
    let body = text.trim_end_matches('=');
    if text.len() - body.len() > 2 || body.len() % 4 == 1 {
        return None;
    }
    let value = |c: u8| ALPHABET.iter().position(|&a| a == c).map(|p| p as u32);
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for chunk in body.as_bytes().chunks(4) {
        let mut acc = 0u32;
        for &c in chunk {
            acc = acc << 6 | value(c)?;
        }
        let bits = chunk.len() * 6;
        let whole = bits / 8;
        let acc = acc >> (bits - whole * 8);
        for i in (0..whole).rev() {
            out.push((acc >> (i * 8)) as u8);
        }
    }
    Some(out)
}

/// The key bytes of a `whsec_<base64>` secret, at least 16 bytes long.
pub fn secret_key(secret: &str) -> Option<Vec<u8>> {
    unbase64(secret.trim().strip_prefix(PREFIX)?).filter(|k| k.len() >= 16)
}

/// The `webhook-signature` value for one attempt.
pub fn signature(key: &[u8], id: &str, timestamp: u64, body: &str) -> String {
    let msg = format!("{id}.{timestamp}.{body}");
    format!("v1,{}", base64(&hmac_sha256(key, msg.as_bytes())))
}

/// True when any `v1,` entry of a space-separated `webhook-signature` header
/// matches (what a receiver checks; used by the tests' receiver).
pub fn verify(key: &[u8], id: &str, timestamp: u64, body: &str, header: &str) -> bool {
    let want = signature(key, id, timestamp, body);
    header
        .split(' ')
        .any(|s| s.starts_with("v1,") && constant_time_eq(s.as_bytes(), want.as_bytes()))
}

/// The `webhook-id` of the delivery of `event_id` to `webhook`: the same for
/// every attempt and every re-delivery of that event, so receivers can
/// de-duplicate on it.
pub fn message_id(webhook: &str, event_id: &str) -> String {
    let digest = Sha256::new()
        .chain_update(webhook.as_bytes())
        .chain_update([0])
        .chain_update(event_id.as_bytes())
        .finalize();
    let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("msg_{hex}")
}
