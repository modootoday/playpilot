use serde_json::{Value, json};

use super::jwks::{Jwks, KeyCache, Lookup, REFETCH_AFTER, TTL};
use super::jwt::{MAX_LEN, parse};
use super::policy::{Policy, SKEW};
use super::{Reject, service_token_id};

/// RS256 token signed by a throwaway 2048-bit test key (`KID`, `MODULUS`);
/// the private half was discarded. Claims: aud `test-aud`, iss
/// `https://team.example.com`, nbf 1700000000, exp 2000000000,
/// common_name `cid-1.access`.
const VECTOR: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6InRlc3Qta2lkIiwidHlwIjoiSldUIn0.eyJhdWQiOlsidGVzdC1hdWQiXSwiaXNzIjoiaHR0cHM6Ly90ZWFtLmV4YW1wbGUuY29tIiwiZXhwIjoyMDAwMDAwMDAwLCJuYmYiOjE3MDAwMDAwMDAsImlhdCI6MTcwMDAwMDAwMCwiY29tbW9uX25hbWUiOiJjaWQtMS5hY2Nlc3MiLCJzdWIiOiIiLCJ0eXBlIjoiYXBwIn0.GDshju8AQqQEsvOvDkZp9pdygiqzg_IJZSAeHAGFNJcgzng0U1Vbtss4ZgWQnYauCdjL2RHWQ9z2MojI0MTHrVcysFJjVjPCybduBGfdcAv6NBQf8fa--AwPNDtxcZfRZkQPRyzC8CGMe4wqzBdIjXnvqI8dxnh4qEWRx-Gg0nL0c-TmRCzLhs3Bib2DEIFvNH1IfzgoyIi0qlAroo2pAA15HjpOzHz9c0Id19ZDQ2yD0bXJoCaF2EUSd2jUcuJ04gT7XwmuGuDVf1gE-UW0JtUzh0tC8Nf0ad-M-1Y4pxBotTV0S4DyMWcp5flB0RsE5NaTyKT_g3W-EqJ3ikxUnA"; // gitleaks:allow (throwaway test key, private half discarded)
const KID: &str = "test-kid";
const MODULUS: &str = "7_Ie4d7GdzHnjuhSsK1IhiQ_2qBjaSSOw4L8a1uVPGSNvera8j9KYGnUqhSsDdY1dgDE4sWNaHpoujh02Xj1KXuSFR86hcE5Zzg40j6rS7zo7FSIeMdgrI2Xe4beW3acKI9VjYnhiwbzO_ql5jzW1JQeqT-B4ka9oL729wlHIsbe88meC_z4zpEavQJv0gcKEVp5m-6lqn4fAvT9Apmd2i15JHeoPpHSgegYbO8uWpkAnA-DKO7NMrdvrOWRdkpG-kLVY_ZyveFqlpcOJgj0O9daPVGB5sp-H1yEuffaQC0X9JrcqlGChhLL3oE__cV1MVPxUuX-zQPG2joT9fwldw";
const DOMAIN: &str = "team.example.com";
const AUD: &str = "test-aud";
const NOW: u64 = 1_800_000_000;

fn encode(raw: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in raw.chunks(3) {
        let acc =
            chunk.iter().fold(0u32, |a, b| (a << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(char::from(ABC[(acc >> (18 - 6 * i) & 63) as usize]));
        }
    }
    out
}

fn token(header: &Value, claims: &Value) -> String {
    format!(
        "{}.{}.{}",
        encode(header.to_string().as_bytes()),
        encode(claims.to_string().as_bytes()),
        encode(&[1; 256])
    )
}

fn header() -> Value {
    json!({"alg": "RS256", "kid": KID})
}

fn claims() -> Value {
    json!({"aud": [AUD], "iss": format!("https://{DOMAIN}"), "exp": NOW + 300,
           "nbf": NOW - 10, "common_name": "cid-1.access"})
}

fn policy() -> Policy {
    Policy::new(Some(DOMAIN), Some(AUD)).unwrap()
}

fn jwks() -> Jwks {
    Jwks::parse(
        &json!({"keys": [{"kty": "RSA", "kid": KID, "alg": "RS256", "use": "sig",
                                  "n": MODULUS, "e": "AQAB"}]})
        .to_string(),
    )
    .unwrap()
}

#[test]
fn parses_the_fixed_vector() {
    let t = parse(VECTOR).unwrap();
    assert_eq!(t.kid, KID);
    assert_eq!(t.signature.len(), 256);
    assert_eq!(t.signing_input, VECTOR.rsplit_once('.').unwrap().0);
    assert_eq!(t.claims.aud, vec![AUD]);
    assert_eq!(t.claims.exp, 2_000_000_000);
    assert_eq!(t.claims.nbf, Some(1_700_000_000));
    assert_eq!(service_token_id(&t.claims), Some("cid-1.access"));
    assert_eq!(policy().check(&t.claims, NOW), Ok(()));
    let key = jwks().find(&t.kid).cloned().unwrap();
    assert_eq!((key.n.as_str(), key.e.as_str()), (MODULUS, "AQAB"));
}

#[test]
fn test_encoder_round_trips() {
    for raw in [&b""[..], b"f", b"fo", b"foo", b"foob", &[0xfb, 0xff]] {
        assert_eq!(super::base64url::decode(&encode(raw)).unwrap(), raw);
    }
}

#[test]
fn refuses_malformed_tokens() {
    let h = header();
    let c = claims();
    let bad = [
        String::new(),
        "a.b".into(),
        "a.b.c.d".into(),
        format!("{}.!!.AQ", encode(h.to_string().as_bytes())),
        token(&json!({"alg": "RS256"}), &c),
        token(&json!({"alg": "RS256", "kid": ""}), &c),
        token(&h, &json!({"iss": "x", "exp": 1})),
        token(&h, &json!({"aud": [1], "iss": "x", "exp": 1})),
        token(&h, &json!({"aud": "a", "exp": 1})),
        token(&h, &json!({"aud": "a", "iss": "x"})),
        token(&h, &json!({"aud": "a", "iss": "x", "exp": 1.5})),
        token(&h, &json!({"aud": "a", "iss": "x", "exp": 1, "nbf": "0"})),
        format!("{}.", VECTOR.rsplit_once('.').unwrap().0),
        "x".repeat(MAX_LEN + 1),
    ];
    for t in bad {
        assert!(
            matches!(parse(&t), Err(Reject::Malformed(_))),
            "{t:.80}: {:?}",
            parse(&t)
        );
    }
}

#[test]
fn refuses_other_algorithms() {
    for alg in ["none", "HS256", "RS384", "ES256", "rs256"] {
        let t = token(&json!({"alg": alg, "kid": KID}), &claims());
        assert_eq!(parse(&t).unwrap_err(), Reject::Algorithm, "{alg}");
    }
}

#[test]
fn checks_issuer_audience_and_window() {
    let p = policy();
    let check = |patch: Value| {
        let mut c = claims();
        for (k, v) in patch.as_object().unwrap() {
            c[k] = v.clone();
        }
        p.check(&parse(&token(&header(), &c)).unwrap().claims, NOW)
    };
    assert_eq!(check(json!({})), Ok(()));
    assert_eq!(check(json!({"aud": AUD})), Ok(()));
    assert_eq!(check(json!({"aud": ["other", AUD]})), Ok(()));
    assert_eq!(check(json!({"aud": ["other"]})), Err(Reject::Audience));
    assert_eq!(check(json!({"aud": []})), Err(Reject::Audience));
    assert_eq!(
        check(json!({"iss": "https://other.example.com"})),
        Err(Reject::Issuer)
    );
    assert_eq!(
        check(json!({"iss": format!("https://{DOMAIN}/")})),
        Err(Reject::Issuer)
    );
    assert_eq!(check(json!({"exp": NOW - SKEW + 1})), Ok(()));
    assert_eq!(check(json!({"exp": NOW - SKEW})), Err(Reject::Expired));
    assert_eq!(check(json!({"nbf": NOW + SKEW})), Ok(()));
    assert_eq!(
        check(json!({"nbf": NOW + SKEW + 1})),
        Err(Reject::NotYetValid)
    );
    let mut no_nbf = claims();
    no_nbf.as_object_mut().unwrap().remove("nbf");
    let t = parse(&token(&header(), &no_nbf)).unwrap();
    assert_eq!(p.check(&t.claims, NOW), Ok(()));
}

#[test]
fn identity_comes_only_from_common_name() {
    for cn in [json!(null), json!("")] {
        let mut c = claims();
        c["common_name"] = cn;
        let t = parse(&token(&header(), &c)).unwrap();
        assert_eq!(service_token_id(&t.claims), None);
    }
}

#[test]
fn policy_needs_both_values_and_a_bare_host() {
    assert!(Policy::new(None, Some(AUD)).is_err());
    assert!(Policy::new(Some(DOMAIN), None).is_err());
    assert!(Policy::new(Some(" "), Some(AUD)).is_err());
    assert!(Policy::new(Some(DOMAIN), Some("")).is_err());
    for bad in [
        "https://team.example.com",
        "team.example.com/",
        "team",
        "team..example.com",
        "-team.example.com",
        "team.example.com:443",
        "user@example.com",
    ] {
        assert!(Policy::new(Some(bad), Some(AUD)).is_err(), "{bad}");
    }
    let p = Policy::new(Some(" Team.Example.com "), Some(AUD)).unwrap();
    assert_eq!(p.issuer(), "https://team.example.com");
    assert_eq!(
        p.certs_url(),
        "https://team.example.com/cdn-cgi/access/certs"
    );
}

#[test]
fn keeps_only_strong_rsa_signing_keys() {
    let key = |patch: Value| {
        let mut k = json!({"kty": "RSA", "kid": "k", "n": MODULUS, "e": "AQAB"});
        for (name, v) in patch.as_object().unwrap() {
            k[name] = v.clone();
        }
        k
    };
    let weak = encode(&[0xc5; 128]);
    let keys = json!({"keys": [
        key(json!({"kid": "ok"})),
        key(json!({"kid": "ec", "kty": "EC"})),
        key(json!({"kid": "enc", "use": "enc"})),
        key(json!({"kid": "rs512", "alg": "RS512"})),
        key(json!({"kid": "weak", "n": weak})),
        key(json!({"kid": "padded", "n": format!("AAAA{}", encode(&[0xc5; 200]))})),
        key(json!({"kid": "bad-e", "e": "A=="})),
        key(json!({"kid": ""})),
    ]});
    let jwks = Jwks::parse(&keys.to_string()).unwrap();
    assert!(jwks.find("ok").is_some());
    for kid in ["ec", "enc", "rs512", "weak", "padded", "bad-e", ""] {
        assert!(jwks.find(kid).is_none(), "{kid}");
    }
    for bad in [
        "[]",
        r#"{"keys": {}}"#,
        r#"{"keys": []}"#,
        r#"{"keys": [{"kty": "EC"}]}"#,
    ] {
        assert!(Jwks::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn caches_keys_per_url_with_a_ttl() {
    let url = policy().certs_url();
    let mut cache = KeyCache::default();
    assert_eq!(cache.lookup(&url, KID, NOW), Lookup::Fetch);
    cache.store(&url, NOW, jwks());
    assert!(matches!(cache.lookup(&url, KID, NOW + 1), Lookup::Hit(_)));
    assert!(matches!(
        cache.lookup(&url, KID, NOW + TTL - 1),
        Lookup::Hit(_)
    ));
    assert_eq!(cache.lookup(&url, KID, NOW + TTL), Lookup::Fetch);
    assert_eq!(cache.lookup(&url, KID, NOW - 1), Lookup::Fetch);
    assert_eq!(
        cache.lookup("https://other.example.com/cdn-cgi/access/certs", KID, NOW),
        Lookup::Fetch
    );
    assert_eq!(cache.lookup(&url, "rotated", NOW + 1), Lookup::Unknown);
    assert_eq!(
        cache.lookup(&url, "rotated", NOW + REFETCH_AFTER),
        Lookup::Fetch
    );
    cache.store(&url, NOW + REFETCH_AFTER, jwks());
    assert_eq!(
        cache.after_fetch(&url, "rotated", NOW + REFETCH_AFTER),
        Lookup::Unknown
    );
}
