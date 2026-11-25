//! HMAC-SHA256 (RFC 2104) on `sha2`, and the `X-Hub-Signature-256` check.

use sha2::{Digest, Sha256};

const BLOCK: usize = 64;

/// HMAC-SHA256 of `msg` under `key`.
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| k.map(|b| b ^ byte);
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(msg)
        .finalize();
    Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

/// Equal-length comparison whose time does not depend on where bytes differ.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn hex32(s: &str) -> Option<[u8; 32]> {
    let b = s.as_bytes();
    if b.len() != 64 {
        return None;
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let mut out = [0u8; 32];
    for (i, pair) in b.chunks(2).enumerate() {
        out[i] = nibble(pair[0])? << 4 | nibble(pair[1])?;
    }
    Some(out)
}

/// True when `header` is `sha256=<hex>` and matches the HMAC of `body`.
pub fn verify(secret: &[u8], body: &[u8], header: Option<&str>) -> bool {
    let Some(given) = header
        .and_then(|h| h.trim().strip_prefix("sha256="))
        .and_then(hex32)
    else {
        return false;
    };
    constant_time_eq(&hmac_sha256(secret, body), &given)
}

/// The header value GitHub would send for `body` (tests and tooling).
pub fn sign(secret: &[u8], body: &[u8]) -> String {
    let mac: String = hmac_sha256(secret, body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256={mac}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn rfc4231_vectors() {
        let mac = hmac_sha256(&[0x0b; 20], b"Hi There");
        assert_eq!(
            hex(&mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        let mac = hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First",
        );
        assert_eq!(
            hex(&mac),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn github_documented_vector() {
        // GitHub's published example: secret "It's a Secret to Everybody",
        // payload "Hello, World!". gitleaks:allow
        let header = "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17"; // gitleaks:allow
        assert!(verify(
            b"It's a Secret to Everybody",
            b"Hello, World!",
            Some(header)
        ));
        assert_eq!(
            sign(b"It's a Secret to Everybody", b"Hello, World!"),
            header
        );
    }

    #[test]
    fn refuses_malformed_or_wrong() {
        let key = b"k";
        let good = sign(key, b"body");
        assert!(verify(key, b"body", Some(&good)));
        assert!(verify(
            key,
            b"body",
            Some(&good.to_ascii_uppercase().replace("SHA256=", "sha256="))
        ));
        assert!(!verify(key, b"body!", Some(&good)));
        assert!(!verify(b"other", b"body", Some(&good)));
        assert!(!verify(key, b"body", None));
        assert!(!verify(
            key,
            b"body",
            Some(&good.replace("sha256=", "sha1="))
        ));
        assert!(!verify(key, b"body", Some(&good[..good.len() - 2])));
        assert!(!verify(key, b"body", Some("sha256=zz")));
        assert!(!constant_time_eq(b"ab", b"abc"));
    }
}
