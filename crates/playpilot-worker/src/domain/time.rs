//! Unix seconds, RFC 3339 UTC text and `<n><s|m|h|d>` durations.

/// RFC 3339 UTC timestamp for `secs` Unix seconds.
pub fn rfc3339(secs: u64) -> String {
    let (z, rem) = ((secs / 86_400) as i64 + 719_468, secs % 86_400);
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// True when `s` has the RFC 3339 date-time shape
/// (`YYYY-MM-DDTHH:MM:SS[.frac](Z|+HH:MM|-HH:MM)`) with in-range fields.
pub fn is_rfc3339(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 20 || !s.is_ascii() {
        return false;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = &s[r];
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let shape = b[4] == b'-'
        && b[7] == b'-'
        && matches!(b[10], b'T' | b't')
        && b[13] == b':'
        && b[16] == b':';
    let fields = (
        num(0..4),
        num(5..7),
        num(8..10),
        num(11..13),
        num(14..16),
        num(17..19),
    );
    let (Some(_), Some(mo), Some(d), Some(h), Some(mi), Some(se)) = fields else {
        return false;
    };
    if !shape || !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return false;
    }
    let mut rest = &s[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return false;
        }
        rest = &frac[digits..];
    }
    match rest.as_bytes() {
        [b'Z' | b'z'] => true,
        [b'+' | b'-', ..] if rest.len() == 6 => {
            rest.as_bytes()[3] == b':' && num_in(&rest[1..3], 23) && num_in(&rest[4..6], 59)
        }
        _ => false,
    }
}

fn num_in(s: &str, max: u32) -> bool {
    s.bytes().all(|c| c.is_ascii_digit()) && s.parse::<u32>().is_ok_and(|n| n <= max)
}

/// Parses `^[0-9]+[smhd]$` into seconds.
pub fn duration(s: &str) -> Result<u64, String> {
    let bad = || format!("`{s}` is not a duration like 30m, 6h or 1d");
    let (n, unit) = s.split_at(s.len().checked_sub(1).ok_or_else(bad)?);
    if n.is_empty() || !n.bytes().all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    let n: u64 = n.parse().map_err(|_| bad())?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => return Err(bad()),
    };
    n.checked_mul(mult).ok_or_else(bad)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_795_651_200), "2026-11-26T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn checks_rfc3339_shape() {
        assert!(is_rfc3339("2026-11-19T10:00:00Z"));
        assert!(is_rfc3339("2026-11-19T10:00:00.123+09:00"));
        assert!(!is_rfc3339("2026-11-19 10:00:00Z"));
        assert!(!is_rfc3339("2026-13-19T10:00:00Z"));
        assert!(!is_rfc3339("2026-11-19T10:00:00"));
        assert!(!is_rfc3339("2026-11-19T10:00:00.Z"));
    }

    #[test]
    fn parses_durations() {
        assert_eq!(duration("90s"), Ok(90));
        assert_eq!(duration("30m"), Ok(1_800));
        assert_eq!(duration("6h"), Ok(21_600));
        assert_eq!(duration("1d"), Ok(86_400));
        for bad in ["", "h", "6", "6w", "-1h", "1.5h", "99999999999999999999d"] {
            assert!(duration(bad).is_err(), "{bad}");
        }
    }
}
