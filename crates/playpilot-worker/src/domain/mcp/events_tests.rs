use serde_json::{Value, json};

use super::*;
use crate::domain::eventlog::store::Memory;
use crate::domain::eventlog::{LogReply, apply};
use crate::domain::mcp::protocol::{Step, handle};
use crate::domain::mcp::tools::Command;
use crate::domain::token::{TokenMap, sha256_hex};

fn store(types: &[&str]) -> Memory {
    let s = Memory::default();
    for (i, ty) in types.iter().enumerate() {
        let json = json!({"specversion": "1.0", "id": format!("d-{i}"), "type": ty}).to_string();
        let cmd = LogCmd::Record {
            id: format!("d-{i}"),
            ty: (*ty).into(),
            json,
        };
        apply(&s, cmd, 100 + i as u64);
    }
    s
}

/// Runs one `events_read` call the way the Worker does: parse, then the
/// `EventsObject` command against the store.
fn call(s: &Memory, args: Value) -> Result<(EventsRead, Vec<Entry>), String> {
    let r = parse(&args)?;
    match apply(s, command(&r), 1_000) {
        LogReply::Events(list) => Ok((r, list)),
        other => Err(format!("{other:?}")),
    }
}

fn seqs(list: &[Entry]) -> Vec<u64> {
    list.iter().map(|e| e.seq).collect()
}

#[test]
fn reads_in_order_after_the_cursor() {
    let s = store(&["com.github.push", "com.github.ping", "com.github.push"]);
    let (_, all) = call(&s, json!({})).unwrap();
    assert_eq!(seqs(&all), [1, 2, 3]);
    let (r, rest) = call(&s, json!({"after": 1})).unwrap();
    assert_eq!(seqs(&rest), [2, 3]);
    assert_eq!(next(r.after, &rest), 3);
    let (r, none) = call(&s, json!({"after": "3"})).unwrap();
    assert!(none.is_empty());
    assert_eq!(next(r.after, &none), 3);
    assert_eq!(render(3, &none), "events: nothing after 3");
}

#[test]
fn limit_defaults_and_is_capped() {
    let types = vec!["com.github.push"; 250];
    let s = store(&types);
    let (r, page) = call(&s, json!({})).unwrap();
    assert_eq!((r.limit, page.len()), (DEFAULT_LIMIT, DEFAULT_LIMIT));
    let (r, page) = call(&s, json!({"limit": 5000})).unwrap();
    assert_eq!((r.limit, page.len()), (MAX_READ, MAX_READ));
    let (_, page) = call(&s, json!({"limit": 2, "after": 10})).unwrap();
    assert_eq!(seqs(&page), [11, 12]);
    assert!(parse(&json!({"limit": 0})).is_err());
    assert!(parse(&json!({"limit": -1})).is_err());
    assert!(parse(&json!({"after": "x"})).is_err());
}

#[test]
fn type_filter_stops_at_dot_boundaries() {
    let s = store(&[
        "com.github.pull_request.opened",
        "com.github.pull_request_review.submitted",
        "com.github.pull_request",
        "com.github.push",
    ]);
    let (_, prs) = call(&s, json!({"type": "com.github.pull_request"})).unwrap();
    assert_eq!(seqs(&prs), [1, 3]);
    let (_, opened) = call(&s, json!({"type": "com.github.pull_request.opened"})).unwrap();
    assert_eq!(seqs(&opened), [1]);
    let (_, partial) = call(&s, json!({"type": "com.github.pu"})).unwrap();
    assert!(partial.is_empty());
    let (_, github) = call(&s, json!({"type": "com.github"})).unwrap();
    assert_eq!(github.len(), 4);
    let (r, _) = call(&s, json!({"type": " "})).unwrap();
    assert_eq!(r.ty_prefix, None);
    for bad in ["com..github", ".com", "com.", "Com.GitHub", "a b", "x%"] {
        assert!(parse(&json!({"type": bad})).is_err(), "{bad}");
    }
    assert!(parse(&json!({"type": 5})).is_err());
}

#[test]
fn structured_page_carries_the_cloud_events_and_cursor() {
    let s = store(&["com.github.ping"]);
    let (r, list) = call(&s, json!({})).unwrap();
    let v = structured(r.after, &list);
    assert_eq!(v["next"], 1);
    assert_eq!(v["events"][0]["seq"], 1);
    assert_eq!(v["events"][0]["type"], "com.github.ping");
    assert_eq!(v["events"][0]["received_at"], "1970-01-01T00:01:40Z");
    assert_eq!(v["events"][0]["event"]["id"], "d-0");
    let text = render(r.after, &list);
    assert!(text.starts_with("1  1970-01-01T00:01:40Z  com.github.ping"));
    assert!(text.ends_with("next: 1"));
}

#[test]
fn is_a_tool_call_that_refuses_unknown_arguments() {
    let body = r#"{"id":9,"method":"tools/call","params":{"name":"events_read","arguments":{"after":4,"type":"com.github.push"}}}"#;
    let Step::Call { call, .. } = handle(body) else {
        panic!("not a call")
    };
    assert_eq!(
        call.command,
        Command::EventsRead(EventsRead {
            after: 4,
            limit: DEFAULT_LIMIT,
            ty_prefix: Some("com.github.push".into())
        })
    );
    let extra = r#"{"id":9,"method":"tools/call","params":{"name":"events_read","arguments":{"holder":"x"}}}"#;
    let Step::Reply(v) = handle(extra) else {
        panic!("not a reply")
    };
    assert_eq!(v["result"]["isError"], true);
}

#[test]
fn needs_a_valid_token_and_a_permitted_as() {
    let shared = sha256_hex("shared-token");
    let map = TokenMap::parse(&format!(r#"{{"{shared}":{{"actors":["alpha","beta"]}}}}"#)).unwrap();
    assert!(map.grant_for(None).is_none());
    assert!(map.grant_for(Some("Bearer wrong")).is_none());
    assert!(map.grant_for(Some("Basic shared-token")).is_none());
    let grant = map.grant_for(Some("Bearer shared-token")).unwrap();
    let caller = Caller::Token(grant.clone());
    assert!(authorize(&caller, None).is_ok());
    assert!(authorize(&caller, Some("beta")).is_ok());
    assert!(authorize(&caller, Some("gamma")).is_err());
}
