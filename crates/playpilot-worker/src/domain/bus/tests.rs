use super::message::render;
use super::store::Memory;
use super::*;

fn env(from: &str, to: &str) -> Envelope {
    Envelope {
        from: from.into(),
        to: to.into(),
        subject: "hello".into(),
        re: Some("o/r#1".into()),
        body: "body text".into(),
    }
}

fn dir() -> Vec<String> {
    vec!["node-a".into(), "node-b".into(), "node-c".into()]
}

#[test]
fn direct_send_lands_in_one_mailbox() {
    let s = Memory::default();
    let sent = send(&s, &dir(), &env("node-a", "node-b"), 10).unwrap();
    assert_eq!(sent, vec![("node-b".to_string(), 1)]);
    assert_eq!(inbox(&s, "node-b", false).unwrap().len(), 1);
    assert!(inbox(&s, "node-a", false).unwrap().is_empty());
}

#[test]
fn broadcast_copies_to_every_other_actor() {
    let s = Memory::default();
    send(&s, &[], &env("node-d", "node-e"), 1).unwrap();
    let sent = send(&s, &dir(), &env("node-a", BROADCAST), 2).unwrap();
    let boxes: Vec<&str> = sent.iter().map(|(b, _)| b.as_str()).collect();
    assert_eq!(boxes, vec!["node-b", "node-c", "node-e"]);
    assert!(inbox(&s, "node-a", true).unwrap().is_empty());
    assert!(s.list(BROADCAST).unwrap().is_empty());
}

#[test]
fn broadcast_needs_someone_else() {
    let s = Memory::default();
    let err = send(&s, &["node-a".into()], &env("node-a", BROADCAST), 1).unwrap_err();
    assert!(err.contains("no other actor"));
}

#[test]
fn inbox_hides_acked_and_sorts() {
    let s = Memory::default();
    send(&s, &dir(), &env("node-a", "node-b"), 20).unwrap();
    send(&s, &dir(), &env("node-c", "node-b"), 10).unwrap();
    let first = inbox(&s, "node-b", false).unwrap();
    assert_eq!(first[0].envelope.from, "node-c");
    mark(&s, "node-b", first[0].id, Receipt::Ack).unwrap();
    assert_eq!(inbox(&s, "node-b", false).unwrap().len(), 1);
    assert_eq!(inbox(&s, "node-b", true).unwrap().len(), 2);
}

#[test]
fn receipts_only_by_the_mailbox_owner() {
    let s = Memory::default();
    let id = send(&s, &dir(), &env("node-a", "node-b"), 1).unwrap()[0].1;
    assert!(mark(&s, "node-a", id, Receipt::Read).is_err());
    assert!(mark(&s, "node-b", 99, Receipt::Read).is_err());
    mark(&s, "node-b", id, Receipt::Read).unwrap();
    let m = &inbox(&s, "node-b", false).unwrap()[0];
    assert!(m.read && !m.acked);
    assert!(render(m).contains(" [read]"));
    assert!(render(m).contains("re: o/r#1"));
}

#[test]
fn validates_envelopes() {
    let s = Memory::default();
    for (from, to) in [("Node A", "node-b"), ("node-a", "x y"), ("all", "node-b")] {
        assert!(send(&s, &dir(), &env(from, to), 1).is_err(), "{from} {to}");
    }
    let mut e = env("node-a", "node-b");
    e.subject = " ".into();
    assert!(send(&s, &dir(), &e, 1).is_err());
    let mut e = env("node-a", "node-b");
    e.body = "x".repeat(message::MAX_BODY + 1);
    assert!(send(&s, &dir(), &e, 1).is_err());
}

#[test]
fn publish_is_idempotent_by_key() {
    let s = Memory::default();
    let events = vec![("src id-1".to_string(), "{}".to_string())];
    let cmd = || BusCmd::Publish {
        events: events.clone(),
    };
    assert_eq!(apply(&s, cmd(), 1), BusReply::Published(1));
    assert_eq!(apply(&s, cmd(), 2), BusReply::Published(0));
}

#[test]
fn apply_reports_errors_as_replies() {
    let s = Memory::default();
    let reply = apply(
        &s,
        BusCmd::Inbox {
            actor: "BAD".into(),
            all: false,
        },
        0,
    );
    assert!(matches!(reply, BusReply::Error(_)));
}

fn send_cmd(from: &str, to: &str, wakeable: &[&str]) -> BusCmd {
    BusCmd::Send {
        envelope: env(from, to),
        directory: dir(),
        wakeable: wakeable.iter().map(|a| a.to_string()).collect(),
    }
}

fn woken(reply: BusReply) -> Vec<(String, usize)> {
    match reply {
        BusReply::Sent { wake, .. } => wake.into_iter().map(|d| (d.actor, d.unacked)).collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn send_wakes_once_per_debounce_window() {
    use crate::domain::wake::DEBOUNCE_SECS;
    let s = Memory::default();
    let w = ["node-b"];
    assert_eq!(
        woken(apply(&s, send_cmd("node-a", "node-b", &w), 100)),
        [("node-b".to_string(), 1)]
    );
    assert!(woken(apply(&s, send_cmd("node-c", "node-b", &w), 200)).is_empty());
    let later = 100 + DEBOUNCE_SECS;
    assert_eq!(
        woken(apply(&s, send_cmd("node-c", "node-b", &w), later)),
        [("node-b".to_string(), 3)]
    );
}

#[test]
fn acking_everything_resets_the_debounce() {
    let s = Memory::default();
    let w = ["node-b"];
    assert_eq!(
        woken(apply(&s, send_cmd("node-a", "node-b", &w), 100)).len(),
        1
    );
    let id = inbox(&s, "node-b", false).unwrap()[0].id;
    mark(&s, "node-b", id, Receipt::Read).unwrap();
    assert!(woken(apply(&s, send_cmd("node-a", "node-b", &w), 110)).is_empty());
    for m in inbox(&s, "node-b", false).unwrap() {
        mark(&s, "node-b", m.id, Receipt::Ack).unwrap();
    }
    assert_eq!(
        woken(apply(&s, send_cmd("node-a", "node-b", &w), 120)).len(),
        1
    );
}

#[test]
fn only_wakeable_recipients_are_woken() {
    let s = Memory::default();
    let got = woken(apply(
        &s,
        send_cmd("node-a", BROADCAST, &["node-c", "node-a"]),
        5,
    ));
    assert_eq!(got, [("node-c".to_string(), 1)]);
    assert!(woken(apply(&s, send_cmd("node-a", "node-b", &[]), 6)).is_empty());
}
