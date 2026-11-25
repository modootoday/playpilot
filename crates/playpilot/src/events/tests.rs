use std::cell::RefCell;

use serde_json::{Value, json};

use super::cli::{DEFAULT_INTERVAL, DEFAULT_LIMIT, Options, parse, tail};
use super::source::{EventSource, Query, WorkerEvents};
use super::*;
use crate::mail::worker::Post;

/// Replays canned `(status, body)` replies (or transport errors) and
/// records each request body.
#[derive(Default)]
struct Script {
    replies: RefCell<Vec<Result<(u16, String), String>>>,
    seen: RefCell<Vec<Value>>,
}

impl Script {
    fn new(replies: Vec<Result<(u16, Value), String>>) -> Self {
        Self {
            replies: RefCell::new(
                replies
                    .into_iter()
                    .rev()
                    .map(|r| r.map(|(s, v)| (s, v.to_string())))
                    .collect(),
            ),
            seen: RefCell::default(),
        }
    }

    fn args(&self, i: usize) -> Value {
        self.seen.borrow()[i]["params"]["arguments"].clone()
    }
}

impl Post for &Script {
    fn post(&self, body: &str) -> Result<(u16, String), String> {
        self.seen
            .borrow_mut()
            .push(serde_json::from_str(body).unwrap());
        self.replies
            .borrow_mut()
            .pop()
            .unwrap_or_else(|| Err("no reply".into()))
    }
}

fn cloud_event(seq: u64, ty: &str, data: Value) -> Value {
    json!({
        "seq": seq,
        "received_at": "2026-11-25T10:00:00Z",
        "type": ty,
        "event": {
            "specversion": "1.0",
            "id": format!("d-{seq}"),
            "source": "https://github.com/octo-org/hello",
            "type": ty,
            "time": "2026-11-25T10:00:01Z",
            "datacontenttype": "application/json",
            "data": data
        }
    })
}

fn page(events: Vec<Value>, next: u64) -> (u16, Value) {
    (
        200,
        json!({"jsonrpc": "2.0", "id": 1, "result": {
            "content": [{"type": "text", "text": "events"}],
            "isError": false,
            "structuredContent": {"events": events, "next": next}
        }}),
    )
}

fn opts(follow: bool) -> Options {
    Options {
        ty: None,
        after: 0,
        limit: 2,
        follow,
        interval: 15,
        json: false,
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn parses_tail_options() {
    let o = parse(&s(&["tail"])).unwrap();
    assert_eq!(
        o,
        Options {
            ty: None,
            after: 0,
            limit: DEFAULT_LIMIT,
            follow: false,
            interval: DEFAULT_INTERVAL,
            json: false,
        }
    );
    let o = parse(&s(&[
        "tail",
        "--type",
        "com.github.push",
        "--after",
        "42",
        "--limit",
        "200",
        "--follow",
        "--interval",
        "5",
        "--json",
    ]))
    .unwrap();
    assert_eq!(o.ty.as_deref(), Some("com.github.push"));
    assert_eq!((o.after, o.limit, o.interval), (42, 200, 5));
    assert!(o.follow && o.json);
    for bad in [
        &["tail", "--limit", "0"][..],
        &["tail", "--limit", "201"],
        &["tail", "--after", "-1"],
        &["tail", "--interval", "5"],
        &["tail", "-f", "--interval", "0"],
        &["tail", "--type"],
        &["tail", "--bogus"],
        &["head"],
        &[],
    ] {
        assert!(parse(&s(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn summarises_projected_data() {
    let pr = json!({"repository": "o/r", "number": 7, "action": "opened", "sender": "octocat",
        "sha": "0123456789abcdef0123456789abcdef01234567", "ref": "refs/heads/main"});
    assert_eq!(
        summary(&pr),
        "o/r#7 opened refs/heads/main@0123456 by octocat"
    );
    assert_eq!(summary(&json!({"repository": "o/r"})), "o/r");
    assert_eq!(summary(&json!({})), "");
    assert_eq!(summary(&Value::Null), "");
}

#[test]
fn reads_one_page_and_formats_lines() {
    let script = Script::new(vec![Ok(page(
        vec![
            cloud_event(
                3,
                "com.github.issues.opened",
                json!({"repository": "o/r", "number": 4, "action": "opened"}),
            ),
            cloud_event(4, "com.github.ping", json!({})),
        ],
        4,
    ))]);
    let src = WorkerEvents::new(&script);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut o = opts(false);
    o.ty = Some("com.github".into());
    o.after = 2;
    tail(&src, &o, &mut out, &mut err, &mut |_| panic!("no sleep")).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "3  2026-11-25T10:00:01Z  com.github.issues.opened  github.com/octo-org/hello  o/r#4 opened\n\
         4  2026-11-25T10:00:01Z  com.github.ping  github.com/octo-org/hello\n"
    );
    let req = &script.seen.borrow()[0];
    assert_eq!(req["method"], "tools/call");
    assert_eq!(req["params"]["name"], "events_read");
    assert_eq!(
        script.args(0),
        json!({"after": 2, "limit": 2, "type": "com.github"})
    );
}

#[test]
fn json_prints_cloud_event_lines() {
    let script = Script::new(vec![Ok(page(
        vec![cloud_event(1, "com.github.ping", json!({"sender": "x"}))],
        1,
    ))]);
    let src = WorkerEvents::new(&script);
    let mut out = Vec::new();
    let mut o = opts(false);
    o.json = true;
    tail(&src, &o, &mut out, &mut Vec::new(), &mut |_| true).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.lines().count(), 1);
    let v: Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(v["specversion"], "1.0");
    assert_eq!(v["id"], "d-1");
    assert_eq!(v["data"]["sender"], "x");
    assert!(script.args(0).get("type").is_none());
}

#[test]
fn one_shot_errors_are_returned() {
    let script = Script::new(vec![Ok((401, json!({})))]);
    let src = WorkerEvents::new(&script);
    let e = tail(
        &src,
        &opts(false),
        &mut Vec::new(),
        &mut Vec::new(),
        &mut |_| true,
    )
    .unwrap_err();
    assert!(e.contains("401"));
    let tool_error = json!({"jsonrpc": "2.0", "id": 1, "result": {
        "content": [{"type": "text", "text": "`limit` must be at least 1"}], "isError": true}});
    let script = Script::new(vec![Ok((200, tool_error))]);
    let src = WorkerEvents::new(&script);
    let e = tail(
        &src,
        &opts(false),
        &mut Vec::new(),
        &mut Vec::new(),
        &mut |_| true,
    )
    .unwrap_err();
    assert!(e.contains("limit"));
    let no_page = json!({"jsonrpc": "2.0", "id": 1, "result": {"content": [], "isError": false}});
    assert!(page_from_structured(&no_page["result"]).is_err());
}

#[test]
fn follow_pages_with_the_cursor_and_retries_errors_with_backoff() {
    let script = Script::new(vec![
        Ok(page(
            vec![
                cloud_event(1, "com.github.push", json!({})),
                cloud_event(2, "com.github.push", json!({})),
            ],
            2,
        )),
        Ok(page(vec![cloud_event(3, "com.github.push", json!({}))], 3)),
        Err("curl failed: could not resolve host".into()),
        Ok((502, json!({}))),
        Ok(page(vec![], 3)),
        Ok(page(vec![cloud_event(5, "com.github.push", json!({}))], 5)),
    ]);
    let src = WorkerEvents::new(&script);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut waits = Vec::new();
    let mut sleep = |secs: u64| {
        waits.push(secs);
        waits.len() < 5
    };
    tail(&src, &opts(true), &mut out, &mut err, &mut sleep).unwrap();
    let seqs: Vec<&str> = std::str::from_utf8(&out)
        .unwrap()
        .lines()
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(seqs, ["1", "2", "3", "5"]);
    assert_eq!(waits, [15, 15, 30, 15, 15]);
    let afters: Vec<Value> = (0..6).map(|i| script.args(i)["after"].clone()).collect();
    assert_eq!(afters, [0, 2, 3, 3, 3, 3]);
    let err = String::from_utf8(err).unwrap();
    assert!(err.contains("could not resolve host (retry in 15s)"));
    assert!(err.contains("HTTP 502 (retry in 30s)"));
}

#[test]
fn backoff_doubles_up_to_five_minutes() {
    assert_eq!(backoff(15, 1), 15);
    assert_eq!(backoff(15, 2), 30);
    assert_eq!(backoff(15, 5), 240);
    assert_eq!(backoff(15, 6), 300);
    assert_eq!(backoff(15, 100), 300);
    assert_eq!(backoff(600, 3), 600);
}

#[test]
fn query_arguments_omit_an_absent_type() {
    let q = Query {
        after: 1,
        limit: 3,
        ty: None,
    };
    assert_eq!(
        Value::Object(source::arguments(&q)),
        json!({"after": 1, "limit": 3})
    );
    struct Empty;
    impl EventSource for Empty {
        fn read(&self, _: &Query) -> Result<Page, String> {
            Ok(Page::default())
        }
    }
    assert_eq!(Empty.read(&q).unwrap().next, 0);
}

#[test]
fn lines_drop_control_characters() {
    let mut e = page_from_structured(
        &page(vec![cloud_event(1, "com.github.ping", json!({}))], 1).1["result"],
    )
    .unwrap()
    .events
    .remove(0);
    e.source = "https://github.com/o\u{1b}[2J".into();
    assert!(!line(&e).contains('\u{1b}'));
}
