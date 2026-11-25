use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::json;

use super::sign::{base64, secret_key, signature, unbase64, verify};
use super::store::Memory;
use super::target::{Target, Url, check, check_url, matching, targets};
use super::*;
use crate::domain::config::Compiled;
use crate::domain::eventlog::store::Memory as Log;
use crate::domain::eventlog::{LogCmd, LogReply, apply};
use crate::domain::ops::{OpsTable, Switches};

/// Test key only, never deployed. gitleaks:allow
const SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw"; // gitleaks:allow
/// The message id of the same reference example. gitleaks:allow
const MSG_ID: &str = "msg_p5jXN8AQM9LWM0D4loKWxJek"; // gitleaks:allow
const T0: u64 = 1_795_651_200;

#[test]
fn signature_matches_the_standard_webhooks_reference_example() {
    // The example from the Standard Webhooks specification and its
    // reference libraries. gitleaks:allow
    let key = secret_key(SECRET).unwrap();
    let body = r#"{"test": 2432232314}"#;
    let want = "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE="; // gitleaks:allow
    assert_eq!(signature(&key, MSG_ID, 1_614_265_330, body), want);
    let header = format!("v1,bm90LXRoaXM= {want}");
    assert!(verify(&key, MSG_ID, 1_614_265_330, body, &header));
    assert!(!verify(&key, MSG_ID, 1_614_265_331, body, want));
}

#[test]
fn base64_round_trips_rfc_4648_vectors() {
    for (raw, enc) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(raw.as_bytes()), enc);
        assert_eq!(unbase64(enc).unwrap(), raw.as_bytes());
    }
    assert!(unbase64("Zm9v-").is_none());
    assert!(
        secret_key(&SECRET["whsec_".len()..]).is_none(),
        "no whsec_ prefix"
    );
    assert!(secret_key("whsec_Zm9v").is_none(), "shorter than 16 bytes");
}

#[test]
fn retry_schedule() {
    let waits: Vec<Option<u64>> = (0..=6).map(retry_after).collect();
    assert_eq!(
        waits,
        [
            None,
            Some(30),
            Some(120),
            Some(600),
            Some(3_600),
            Some(21_600),
            None
        ]
    );
    assert_eq!(MAX_ATTEMPTS, 6);
}

fn target(name: &str, url: &str, events: &[&str]) -> Target {
    Target {
        name: name.into(),
        url: Url::Literal(url.into()),
        secret: format!("PLAYPILOT_WEBHOOK_SECRET_{}", name.to_ascii_uppercase()),
        events: events.iter().map(|e| e.to_string()).collect(),
    }
}

/// A receiver on the far side of a fake transport: it checks every
/// signature with the shared key and answers from a script (default 204).
#[derive(Default)]
struct Receiver {
    answers: RefCell<Vec<Outcome>>,
    seen: RefCell<Vec<(String, u64, String)>>,
}

impl Receiver {
    fn receive(&self, r: &Outgoing) -> Outcome {
        let h = |n: &str| {
            r.headers
                .iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        let (id, ts) = (h("webhook-id"), h("webhook-timestamp").parse().unwrap());
        let key = secret_key(SECRET).unwrap();
        assert!(verify(&key, &id, ts, &r.body, &h("webhook-signature")));
        assert_eq!(h("content-type"), "application/json");
        self.seen.borrow_mut().push((id, ts, r.body.clone()));
        let mut a = self.answers.borrow_mut();
        if a.is_empty() {
            Outcome::Status(204)
        } else {
            a.remove(0)
        }
    }
}

struct Harness {
    log: Log,
    outbox: Memory,
    targets: Vec<Target>,
    secrets: BTreeMap<String, String>,
    receiver: Receiver,
}

impl Harness {
    fn new(targets: Vec<Target>) -> Self {
        let secrets = targets
            .iter()
            .map(|t| (t.secret.clone(), SECRET.to_string()))
            .collect();
        Self {
            log: Log::default(),
            outbox: Memory::default(),
            targets,
            secrets,
            receiver: Receiver::default(),
        }
    }

    /// The inbound path: store the event and queue it for matching targets.
    fn inbound(&self, id: &str, ty: &str, now: u64) -> LogReply {
        let json = json!({"specversion": "1.0", "id": id, "type": ty}).to_string();
        let cmd = LogCmd::Record {
            id: id.into(),
            ty: ty.into(),
            json,
            webhooks: matching(&self.targets, ty),
        };
        apply(&self.log, &self.outbox, cmd, now)
    }

    /// One alarm run, as `EventsObject` does it.
    fn alarm(&self, now: u64) -> Vec<Delivery> {
        let lookup = |n: &str| self.secrets.get(n).cloned();
        let b = Bindings {
            targets: &self.targets,
            secret: &lookup,
        };
        claim(&self.outbox, &b, now, BATCH)
            .unwrap()
            .into_iter()
            .map(|(d, r)| {
                let o = self.receiver.receive(&r);
                finish(&self.outbox, d, &r, o, now).unwrap()
            })
            .collect()
    }

    fn rows(&self) -> Vec<Delivery> {
        self.outbox.rows.borrow().clone()
    }

    fn sent(&self) -> usize {
        self.receiver.seen.borrow().len()
    }
}

#[test]
fn delivers_a_matching_event_signed_with_the_stored_body() {
    let h = Harness::new(vec![
        target(
            "prs",
            "https://hooks.example.com/in",
            &["com.github.pull_request"],
        ),
        target(
            "pings",
            "https://hooks.example.com/ping",
            &["com.github.ping"],
        ),
    ]);
    assert_eq!(
        h.inbound("d-1", "com.github.pull_request.opened", T0),
        LogReply::Recorded { seq: 1 }
    );
    let done = h.alarm(T0);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].webhook, "prs");
    assert_eq!(done[0].state, State::Delivered);
    assert_eq!(done[0].last_status, Some(204));
    let seen = h.receiver.seen.borrow();
    assert_eq!(seen[0].0, sign::message_id("prs", "d-1"));
    assert_eq!(seen[0].1, T0);
    assert_eq!(seen[0].2, h.log.events.borrow()[0].json);
    assert!(h.alarm(T0 + 1).is_empty());
}

#[test]
fn retries_on_the_schedule_with_one_webhook_id_then_gives_up() {
    let h = Harness::new(vec![target(
        "prs",
        "https://hooks.example.com/in",
        &["com.github"],
    )]);
    *h.receiver.answers.borrow_mut() = vec![
        Outcome::Status(500),
        Outcome::Transport("timed out after 10 s".into()),
        Outcome::Status(429),
        Outcome::Status(302),
        Outcome::Status(503),
        Outcome::Status(500),
    ];
    h.inbound("d-1", "com.github.push", T0);
    let mut now = T0;
    for (n, wait) in SCHEDULE.iter().enumerate() {
        let d = h.alarm(now).pop().unwrap();
        assert_eq!((d.state, d.attempts), (State::Pending, n as u32 + 1));
        assert_eq!(d.next_at, now + wait);
        assert!(h.alarm(now + wait - 1).is_empty(), "not due before {wait}s");
        now += wait;
    }
    let last = h.alarm(now).pop().unwrap();
    assert_eq!(last.state, State::Failed);
    assert_eq!(last.attempts, MAX_ATTEMPTS);
    assert_eq!(last.last_status, Some(500));
    assert!(last.detail.unwrap().starts_with("gave up after 6 attempts"));
    let seen = h.receiver.seen.borrow();
    assert_eq!(seen.len(), 6);
    assert!(seen.iter().all(|(id, _, _)| *id == seen[0].0));
    let stamps: Vec<u64> = seen.iter().map(|s| s.1).collect();
    assert_eq!(
        stamps,
        [T0, T0 + 30, T0 + 150, T0 + 750, T0 + 4_350, T0 + 25_950]
    );
    assert!(h.alarm(now + 86_400).is_empty());
}

#[test]
fn a_redelivered_inbound_event_or_a_lost_outcome_never_adds_a_webhook_id() {
    let h = Harness::new(vec![target(
        "prs",
        "https://hooks.example.com/in",
        &["com.github.push"],
    )]);
    h.inbound("d-1", "com.github.push", T0);
    assert_eq!(
        h.inbound("d-1", "com.github.push", T0 + 5),
        LogReply::Duplicate
    );
    // Even an event the log no longer remembers is queued once per target.
    assert_eq!(
        enqueue(
            &h.outbox,
            &["prs".into()],
            "d-1",
            "com.github.push",
            "{}",
            T0 + 9
        )
        .unwrap(),
        0
    );
    assert_eq!(h.rows().len(), 1);
    // An attempt whose outcome is never recorded is retried with the same id.
    let lookup = |n: &str| h.secrets.get(n).cloned();
    let b = Bindings {
        targets: &h.targets,
        secret: &lookup,
    };
    let (d, first) = claim(&h.outbox, &b, T0, BATCH).unwrap().pop().unwrap();
    assert_eq!((d.attempts, d.next_at), (1, T0 + 30));
    assert!(claim(&h.outbox, &b, T0 + 29, BATCH).unwrap().is_empty());
    let (_, second) = claim(&h.outbox, &b, T0 + 30, BATCH).unwrap().pop().unwrap();
    let id = |r: &Outgoing| {
        r.headers
            .iter()
            .find(|(k, _)| *k == "webhook-id")
            .unwrap()
            .1
            .clone()
    };
    assert_eq!(id(&first), id(&second));
    assert_ne!(
        first.headers, second.headers,
        "timestamp and signature are per attempt"
    );
}

#[test]
fn an_unrecorded_last_attempt_gives_up() {
    let h = Harness::new(vec![target(
        "prs",
        "https://hooks.example.com/in",
        &["com"],
    )]);
    h.inbound("d-1", "com.github.push", T0);
    let mut d = h.rows().pop().unwrap();
    d.attempts = MAX_ATTEMPTS;
    d.next_at = T0;
    h.outbox.save(&d).unwrap();
    assert!(h.alarm(T0).is_empty());
    let d = h.rows().pop().unwrap();
    assert_eq!(d.state, State::Failed);
    assert_eq!(h.sent(), 0);
}

#[test]
fn gone_disables_the_target_until_its_url_changes() {
    let mut h = Harness::new(vec![target(
        "prs",
        "https://hooks.example.com/in",
        &["com"],
    )]);
    *h.receiver.answers.borrow_mut() = vec![Outcome::Status(410)];
    h.inbound("d-1", "com.github.push", T0);
    h.inbound("d-2", "com.github.push", T0);
    let done = h.alarm(T0);
    assert_eq!(done.len(), 1, "one delivery per target and run");
    assert!(h.alarm(T0).is_empty());
    assert_eq!(h.sent(), 1, "the second delivery is not sent after the 410");
    assert!(done[0].detail.as_deref().unwrap().starts_with("410 Gone"));
    let rows = h.rows();
    assert!(rows.iter().all(|d| d.state == State::Failed));
    assert!(rows[1].detail.as_deref().unwrap().contains("disabled"));
    h.inbound("d-3", "com.github.push", T0 + 1);
    h.alarm(T0 + 1);
    assert_eq!(h.sent(), 1);
    h.targets[0].url = Url::Literal("https://hooks.example.com/v2".into());
    h.inbound("d-4", "com.github.push", T0 + 2);
    assert_eq!(h.alarm(T0 + 2)[0].state, State::Delivered);
}

#[test]
fn a_missing_or_malformed_secret_fails_closed_without_sending() {
    let mut h = Harness::new(vec![target(
        "prs",
        "https://hooks.example.com/in",
        &["com"],
    )]);
    h.secrets.clear();
    h.inbound("d-1", "com.github.push", T0);
    assert!(h.alarm(T0).is_empty());
    let d = h.rows().pop().unwrap();
    assert_eq!((d.state, d.attempts), (State::Failed, 0));
    let why = d.detail.unwrap();
    assert!(
        why.contains("PLAYPILOT_WEBHOOK_SECRET_PRS is not set"),
        "{why}"
    );
    h.secrets
        .insert("PLAYPILOT_WEBHOOK_SECRET_PRS".into(), "not-a-whsec".into());
    h.inbound("d-2", "com.github.push", T0);
    h.alarm(T0);
    let d = h.rows().pop().unwrap();
    assert_eq!(d.state, State::Failed);
    assert!(
        !d.detail.unwrap().contains("not-a-whsec"),
        "no secret text in state"
    );
    assert_eq!(h.sent(), 0);
}

#[test]
fn a_target_removed_from_the_config_or_a_bad_url_binding_fails_unsent() {
    let mut h = Harness::new(vec![target(
        "prs",
        "https://hooks.example.com/in",
        &["com"],
    )]);
    h.inbound("d-1", "com.github.push", T0);
    h.targets[0].url = Url::Secret("PLAYPILOT_WEBHOOK_URL_PRS".into());
    h.secrets.insert(
        "PLAYPILOT_WEBHOOK_URL_PRS".into(),
        "https://127.0.0.1/x".into(),
    );
    h.alarm(T0);
    assert!(h.rows()[0].detail.as_deref().unwrap().contains("private"));
    h.targets.clear();
    h.inbound("d-2", "com.github.push", T0);
    let d = Delivery::new("prs", "d-3", "com.github.push", "{}", T0);
    assert!(h.outbox.enqueue(&d).unwrap());
    h.alarm(T0);
    assert!(h.rows().iter().all(|d| d.state == State::Failed));
    assert_eq!(h.sent(), 0);
}

#[test]
fn reads_deliveries_after_a_cursor_by_webhook_without_bodies() {
    let h = Harness::new(vec![
        target("a", "https://a.example.com/", &["com.github.push"]),
        target("b", "https://b.example.com/", &["com.github"]),
    ]);
    h.inbound("d-1", "com.github.push", T0);
    h.inbound("d-2", "com.github.ping", T0);
    let reply = apply(
        &h.log,
        &h.outbox,
        LogCmd::Deliveries {
            after: 0,
            limit: 10,
            webhook: None,
        },
        T0,
    );
    let LogReply::Deliveries(all) = reply else {
        panic!("{reply:?}")
    };
    let seqs: Vec<(u64, &str)> = all.iter().map(|d| (d.seq, d.webhook.as_str())).collect();
    assert_eq!(seqs, [(1, "a"), (2, "b"), (3, "b")]);
    assert!(all.iter().all(|d| d.body.is_empty()));
    let b = read(&h.outbox, 2, 10, Some("b")).unwrap();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].event_id, "d-2");
}

#[test]
fn filters_match_whole_dot_separated_prefixes() {
    let t = target(
        "prs",
        "https://hooks.example.com/",
        &["com.github.pull_request", "com.github.ping"],
    );
    assert!(t.wants("com.github.pull_request"));
    assert!(t.wants("com.github.pull_request.opened"));
    assert!(t.wants("com.github.ping"));
    assert!(!t.wants("com.github.pull_request_review.submitted"));
    assert!(!t.wants("com.github.push"));
    assert!(!t.wants("com.github"));
}

#[test]
fn reads_outbound_webhooks_from_the_compiled_config() {
    let spec = |name: &str, extra: serde_json::Value| {
        let mut s = json!({
            "direction": "outbound",
            "url": "https://hooks.example.com/in",
            "signing": {"scheme": "standard-webhooks", "secretRef": format!("PLAYPILOT_WEBHOOK_SECRET_{name}")},
            "format": "cloudevents/1.0",
            "events": ["com.github.push"]
        });
        for (k, v) in extra.as_object().unwrap() {
            s[k] = v.clone();
        }
        s
    };
    let res = |name: &str, extra| json!({"kind": "Webhook", "metadata": {"name": name}, "spec": spec(&name.to_ascii_uppercase().replace('-', "_"), extra)});
    let body = json!({
        "apiVersion": "playpilot.io/v1",
        "rev": "c".repeat(40),
        "resources": [
            res("ok", json!({})),
            res("secret-url", json!({"url": {"secretRef": "PLAYPILOT_WEBHOOK_URL_X"}})),
            res("inbound", json!({"direction": "inbound"})),
            res("paused", json!({"ops": {"paused": true}})),
            res("http", json!({"url": "http://hooks.example.com/"})),
            res("other-secret", json!({"signing": {"scheme": "standard-webhooks", "secretRef": "PLAYPILOT_GITHUB_WEBHOOK_SECRET"}})),
            res("bad-filter", json!({"events": ["com..github"]})),
            res("no-filter", json!({"events": []})),
            res("later", json!({})),
            {"kind": "Lane", "metadata": {"name": "build"}, "spec": {}}
        ]
    });
    let c = Compiled::parse(&body.to_string()).unwrap();
    let sw = Switches {
        file: c.file_ops(),
        worker: OpsTable::parse(r#"{"Webhook/later":{"paused":true}}"#).unwrap(),
    };
    let names: Vec<String> = targets(&c, &sw).into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["ok", "secret-url"]);
}

#[test]
fn urls_must_be_public_https() {
    for ok in [
        "https://hooks.example.com/in",
        "https://hooks.example.com:8443/in?x=1",
        "https://203.0.113.9/in",
        "https://example.com.",
    ] {
        assert_eq!(check_url(ok), Ok(()), "{ok}");
    }
    for bad in [
        "http://hooks.example.com/",
        "https://user:pw@example.com/",
        "https://localhost/",
        "https://api.localhost/",
        "https://printer.local/",
        "https://svc.internal/",
        "https://router.home.arpa/",
        "https://intranet/",
        "https://127.0.0.1/",
        "https://10.1.2.3/",
        "https://172.20.0.1/",
        "https://192.168.1.1/",
        "https://169.254.169.254/",
        "https://100.64.0.1/",
        "https://0.0.0.0/",
        "https://[::1]/",
        "https://hooks.example.com:x/",
        "https://hooks example.com/",
        "https:///path",
        "https://1.2.3/",
    ] {
        assert!(check_url(bad).is_err(), "{bad}");
    }
    let mut t = target("x", "https://hooks.example.com/", &["com.github"]);
    assert!(check(&t).is_ok());
    t.secret = "OTHER_SECRET".into();
    assert!(check(&t).is_err());
}
