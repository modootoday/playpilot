//! Unpadded base64url (RFC 4648 section 5), the encoding of JWT segments and
//! JWK numbers. Padding, other alphabets and non-zero trailing bits are refused.

fn value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    }
}

pub fn decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for &c in chunk {
            acc = (acc << 6) | value(c)?;
        }
        let bits = chunk.len() * 6;
        let whole = bits / 8;
        let spare = bits - whole * 8;
        if acc & ((1 << spare) - 1) != 0 {
            return None;
        }
        let acc = acc >> spare;
        for i in (0..whole).rev() {
            out.push((acc >> (i * 8)) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn decodes_rfc_4648_vectors() {
        for (enc, raw) in [
            ("", ""),
            ("Zg", "f"),
            ("Zm8", "fo"),
            ("Zm9v", "foo"),
            ("Zm9vYg", "foob"),
            ("Zm9vYmE", "fooba"),
            ("Zm9vYmFy", "foobar"),
        ] {
            assert_eq!(decode(enc).unwrap(), raw.as_bytes(), "{enc}");
        }
        assert_eq!(decode("-_8").unwrap(), [0xfb, 0xff]);
    }

    #[test]
    fn refuses_other_forms() {
        for bad in [
            "Zg==",
            "Zm9v+",
            "Zm9v/",
            "Z",
            "Zm9vY",
            "Zh",
            "Zm9 v",
            "Zm9\u{e9}",
        ] {
            assert!(decode(bad).is_none(), "{bad}");
        }
    }
}
