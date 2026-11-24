//! Worker-backed claims: the lease decides, the item mirrors it.

use std::cell::RefCell;

use super::lease::{Acquired, Grant, Leases};
use super::tracker::{Comment, Tracker};
use super::{LABEL, Outcome, claim_leased, holder, release_expired, release_leased, sweep_expired};

const ITEM: &str = "modootoday/kinekino#60";

#[derive(Default)]
struct Item {
    comments: RefCell<Vec<Comment>>,
    labels: RefCell<Vec<String>>,
    clock: RefCell<String>,
    /// A GitHub-only rival claim that lands just before ours.
    rival: Option<String>,
    /// Items `labelled` returns.
    labelled: Vec<String>,
}

impl Item {
    fn at(t: &str) -> Self {
        let i = Item::default();
        *i.clock.borrow_mut() = t.into();
        i
    }
    fn push(&self, body: &str, at: &str) {
        self.comments.borrow_mut().push(Comment {
            body: body.into(),
            created_at: at.into(),
        });
    }
    fn bodies(&self) -> Vec<String> {
        self.comments
            .borrow()
            .iter()
            .map(|c| c.body.clone())
            .collect()
    }
    fn last(&self) -> String {
        self.bodies().last().cloned().unwrap_or_default()
    }
    fn labelled(&self) -> bool {
        self.labels.borrow().iter().any(|l| l == LABEL)
    }
}

impl Tracker for Item {
    fn comments(&self, _: &str) -> Result<Vec<Comment>, String> {
        Ok(self.comments.borrow().clone())
    }
    fn comment(&self, _: &str, body: &str) -> Result<(), String> {
        let at = self.clock.borrow().clone();
        if let (Some(r), true) = (&self.rival, body.starts_with("claim:")) {
            self.push(&format!("claim: {r} {at}"), &at);
        }
        self.push(body, &at);
        Ok(())
    }
    fn add_label(&self, _: &str, l: &str) -> Result<(), String> {
        if !self.labels.borrow().iter().any(|x| x == l) {
            self.labels.borrow_mut().push(l.into());
        }
        Ok(())
    }
    fn remove_label(&self, _: &str, l: &str) -> Result<(), String> {
        self.labels.borrow_mut().retain(|x| x != l);
        Ok(())
    }
    fn labelled(&self, _: &str, _: &str) -> Result<Vec<String>, String> {
        Ok(self.labelled.clone())
    }
}

/// The Worker's lease book for one item, as seen through `task_claim`.
struct Book {
    /// The credential's claim id.
    me: String,
    /// The open lease: holder and lease id.
    open: RefCell<Option<(String, String)>>,
    until: String,
    calls: RefCell<Vec<String>>,
    seq: RefCell<u32>,
}

impl Book {
    fn new(me: &str, until: &str) -> Self {
        Book {
            me: me.into(),
            open: RefCell::new(None),
            until: until.into(),
            calls: RefCell::new(Vec::new()),
            seq: RefCell::new(0),
        }
    }
    fn held_by(self, holder: &str, lease: &str) -> Self {
        *self.open.borrow_mut() = Some((holder.into(), lease.into()));
        self
    }
    fn open(&self) -> Option<(String, String)> {
        self.open.borrow().clone()
    }
}

impl Leases for Book {
    fn acquire(&self, _: &str, lane: &str, hours: u64) -> Result<Acquired, String> {
        self.calls
            .borrow_mut()
            .push(format!("acquire {lane} {hours}h"));
        let current = self.open();
        let lease = match current {
            Some((h, _)) if h != self.me => {
                return Ok(Acquired::Refused(format!(
                    "the item has an open lease held by {h}"
                )));
            }
            Some((_, id)) => id,
            None => {
                *self.seq.borrow_mut() += 1;
                format!("l{}", self.seq.borrow())
            }
        };
        *self.open.borrow_mut() = Some((self.me.clone(), lease.clone()));
        Ok(Acquired::Granted(Grant {
            holder: self.me.clone(),
            lease,
            lane: lane.into(),
            until: self.until.clone(),
        }))
    }
    fn release(&self, _: &str) -> Result<(), String> {
        self.calls.borrow_mut().push("release".into());
        match self.open() {
            Some((h, _)) if h != self.me => Err("the lease belongs to another holder".into()),
            _ => {
                *self.open.borrow_mut() = None;
                Ok(())
            }
        }
    }
}

#[test]
fn a_lease_is_mirrored_as_comment_and_label() {
    let t = Item::at("2026-11-24T10:00:00Z");
    let w = Book::new("node-a", "2026-11-24T16:00:00Z");
    let o = claim_leased(&t, &w, ITEM, "node-a", "build", "2026-11-24T10:00:00Z", 6).unwrap();
    assert_eq!(o, Outcome::Claimed { took_over: None });
    assert_eq!(w.calls.borrow().as_slice(), ["acquire build 6h"]);
    assert_eq!(
        t.last(),
        "claim: node-a 2026-11-24T10:00:00Z\nlease: l1 until 2026-11-24T16:00:00Z"
    );
    assert!(t.labelled());
    let h = holder(&t.comments(ITEM).unwrap()).unwrap();
    assert_eq!(h.id, "node-a");
    let m = h.lease.unwrap();
    assert_eq!(
        (m.id.as_str(), m.until.as_str()),
        ("l1", "2026-11-24T16:00:00Z")
    );
}

#[test]
fn claiming_again_renews_and_updates_the_mirror() {
    let t = Item::at("2026-11-24T10:00:00Z");
    let w = Book::new("node-a", "2026-11-24T16:00:00Z");
    claim_leased(&t, &w, ITEM, "node-a", "build", "2026-11-24T10:00:00Z", 6).unwrap();
    let w = Book {
        until: "2026-11-24T18:00:00Z".into(),
        ..w
    };
    *t.clock.borrow_mut() = "2026-11-24T12:00:00Z".into();
    let o = claim_leased(&t, &w, ITEM, "node-a", "build", "2026-11-24T12:00:00Z", 6).unwrap();
    assert_eq!(o, Outcome::AlreadyYours);
    assert_eq!(t.last(), "lease: l1 until 2026-11-24T18:00:00Z");
    let h = holder(&t.comments(ITEM).unwrap()).unwrap();
    assert_eq!(h.lease.unwrap().until, "2026-11-24T18:00:00Z");
    assert_eq!(h.last_activity, "2026-11-24T12:00:00Z");
}

#[test]
fn an_open_lease_elsewhere_refuses_without_touching_the_item() {
    let t = Item::at("2026-11-24T10:00:00Z");
    let w = Book::new("node-b", "2026-11-24T16:00:00Z").held_by("node-a", "l7");
    let o = claim_leased(&t, &w, ITEM, "node-b", "build", "2026-11-24T10:00:00Z", 6).unwrap();
    assert_eq!(
        o,
        Outcome::Refused("the item has an open lease held by node-a".into())
    );
    assert!(t.bodies().is_empty());
    assert!(!t.labelled());
}

#[test]
fn a_mirrored_claim_whose_lease_is_gone_is_taken_over() {
    let t = Item::at("2026-11-24T17:00:00Z");
    t.push(
        "claim: node-a 2026-11-24T10:00:00Z\nlease: l1 until 2026-11-24T16:00:00Z",
        "2026-11-24T10:00:00Z",
    );
    t.labels.borrow_mut().push(LABEL.into());
    // The Worker expired node-a's lease, so it grants node-b a new one.
    let w = Book::new("node-b", "2026-11-24T23:00:00Z");
    let o = claim_leased(&t, &w, ITEM, "node-b", "build", "2026-11-24T17:00:00Z", 6).unwrap();
    assert_eq!(
        o,
        Outcome::Claimed {
            took_over: Some("node-a".into())
        }
    );
    let bodies = t.bodies();
    assert!(
        bodies[1].starts_with("release: expired lease\n"),
        "{bodies:?}"
    );
    assert_eq!(holder(&t.comments(ITEM).unwrap()).unwrap().id, "node-b");
}

#[test]
fn a_fresh_github_only_claim_still_wins_and_the_lease_is_returned() {
    let t = Item::at("2026-11-24T10:30:00Z");
    t.push("claim: node-a 2026-11-24T10:00:00Z", "2026-11-24T10:00:00Z");
    let w = Book::new("node-b", "2026-11-24T16:30:00Z");
    match claim_leased(&t, &w, ITEM, "node-b", "build", "2026-11-24T10:30:00Z", 6).unwrap() {
        Outcome::HeldBy(h) => assert_eq!(h.id, "node-a"),
        o => panic!("{o:?}"),
    }
    assert_eq!(w.open(), None, "the lease must be given back");
    assert_eq!(t.bodies().len(), 1);
}

#[test]
fn a_stale_github_only_claim_is_taken_over() {
    let t = Item::at("2026-11-24T17:00:00Z");
    t.push("claim: node-a 2026-11-24T09:00:00Z", "2026-11-24T09:00:00Z");
    let w = Book::new("node-b", "2026-11-24T23:00:00Z");
    let o = claim_leased(&t, &w, ITEM, "node-b", "build", "2026-11-24T17:00:00Z", 6).unwrap();
    assert_eq!(
        o,
        Outcome::Claimed {
            took_over: Some("node-a".into())
        }
    );
    assert!(t.bodies()[1].starts_with("release: stale claim\n"));
}

#[test]
fn losing_the_race_gives_the_lease_back() {
    let t = Item {
        rival: Some("node-c".into()),
        ..Item::at("2026-11-24T12:00:00Z")
    };
    let w = Book::new("node-d", "2026-11-24T18:00:00Z");
    let o = claim_leased(&t, &w, ITEM, "node-d", "build", "2026-11-24T12:00:00Z", 6).unwrap();
    assert_eq!(o, Outcome::LostRace("node-c".into()));
    assert_eq!(w.open(), None);
    assert_eq!(holder(&t.comments(ITEM).unwrap()).unwrap().id, "node-c");
}

#[test]
fn the_credential_decides_the_claim_id() {
    let t = Item::at("2026-11-24T10:00:00Z");
    let w = Book::new("node-a", "2026-11-24T16:00:00Z");
    let e = claim_leased(&t, &w, ITEM, "node-z", "build", "2026-11-24T10:00:00Z", 6).unwrap_err();
    assert!(e.contains("node-a") && e.contains("node-z"), "{e}");
    assert_eq!(w.open(), None);
    assert!(t.bodies().is_empty());
    assert!(claim_leased(&t, &w, ITEM, "Bad Id", "build", "2026-11-24T10:00:00Z", 6).is_err());
}

#[test]
fn release_frees_lease_and_label() {
    let t = Item::at("2026-11-24T10:00:00Z");
    let w = Book::new("node-a", "2026-11-24T16:00:00Z");
    claim_leased(&t, &w, ITEM, "node-a", "build", "2026-11-24T10:00:00Z", 6).unwrap();
    release_leased(&t, &w, ITEM, "node-a", Some("ran: cargo test; pass")).unwrap();
    assert_eq!(w.open(), None);
    assert!(!t.labelled());
    assert_eq!(t.last(), "release: node-a\nran: cargo test; pass");
    assert_eq!(holder(&t.comments(ITEM).unwrap()), None);
}

#[test]
fn release_by_someone_else_changes_nothing() {
    let t = Item::at("2026-11-24T10:00:00Z");
    let w = Book::new("node-a", "2026-11-24T16:00:00Z");
    claim_leased(&t, &w, ITEM, "node-a", "build", "2026-11-24T10:00:00Z", 6).unwrap();
    let other = Book::new("node-b", "x").held_by("node-a", "l1");
    assert!(release_leased(&t, &other, ITEM, "node-b", None).is_err());
    assert!(other.open().is_some());
    assert!(t.labelled());
}

#[test]
fn an_expired_lease_releases_the_item() {
    let t = Item::at("2026-11-24T16:00:01Z");
    t.push(
        "claim: node-a 2026-11-24T10:00:00Z\nlease: l1 until 2026-11-24T16:00:00Z",
        "2026-11-24T10:00:00Z",
    );
    t.labels.borrow_mut().push(LABEL.into());
    assert_eq!(
        release_expired(&t, ITEM, "2026-11-24T15:59:59Z").unwrap(),
        None,
        "not yet expired"
    );
    assert!(t.labelled());
    assert_eq!(
        release_expired(&t, ITEM, "2026-11-24T16:00:01Z").unwrap(),
        Some("node-a".into())
    );
    assert!(!t.labelled());
    assert!(t.last().starts_with("release: expired lease\n"));
    assert_eq!(holder(&t.comments(ITEM).unwrap()), None);
    assert_eq!(
        release_expired(&t, ITEM, "2026-11-24T17:00:00Z").unwrap(),
        None
    );
}

#[test]
fn a_renewed_mirror_is_not_expired() {
    let t = Item::at("2026-11-24T17:00:00Z");
    t.push(
        "claim: node-a 2026-11-24T10:00:00Z\nlease: l1 until 2026-11-24T16:00:00Z",
        "2026-11-24T10:00:00Z",
    );
    t.push(
        "lease: l1 until 2026-11-24T20:00:00Z",
        "2026-11-24T14:00:00Z",
    );
    assert_eq!(
        release_expired(&t, ITEM, "2026-11-24T17:00:00Z").unwrap(),
        None
    );
}

#[test]
fn github_only_claims_have_no_lease_to_expire() {
    let t = Item::at("2026-11-25T10:00:00Z");
    t.push("claim: node-a 2026-11-24T10:00:00Z", "2026-11-24T10:00:00Z");
    assert_eq!(
        release_expired(&t, ITEM, "2026-11-25T10:00:00Z").unwrap(),
        None
    );
}

#[test]
fn sweep_releases_every_expired_item_in_a_repo() {
    let t = Item {
        labelled: vec![ITEM.into()],
        ..Item::at("2026-11-24T17:00:00Z")
    };
    t.push(
        "claim: node-a 2026-11-24T10:00:00Z\nlease: l1 until 2026-11-24T16:00:00Z",
        "2026-11-24T10:00:00Z",
    );
    t.labels.borrow_mut().push(LABEL.into());
    let done = sweep_expired(&t, "modootoday/kinekino", "2026-11-24T17:00:00Z").unwrap();
    assert_eq!(done, vec![(ITEM.to_string(), "node-a".to_string())]);
    assert!(!t.labelled());
}

#[test]
fn worker_texts_parse() {
    use super::lease::{parse_grant, parse_refusal};
    let g = parse_grant(
        "claimed modootoday/kinekino#60 as node-a (lease l6745-3, lane build, until 2026-11-24T16:00:00Z)",
    )
    .unwrap();
    assert_eq!(
        g,
        Grant {
            holder: "node-a".into(),
            lease: "l6745-3".into(),
            lane: "build".into(),
            until: "2026-11-24T16:00:00Z".into(),
        }
    );
    assert!(parse_grant("claimed o/r#1 as x").is_none());
    assert!(parse_grant("claimed o/r#1 as x (lease l1, lane b, until soon)").is_none());
    assert_eq!(
        parse_refusal("not claimed o/r#1: the item has an open lease held by node-b"),
        Some("the item has an open lease held by node-b".into())
    );
    assert_eq!(parse_refusal("the mail Worker answered HTTP 500"), None);
}

mod worker_adapter {
    use std::cell::RefCell;

    use serde_json::{Value, json};

    use super::super::lease::{Acquired, Leases};
    use super::super::worker::WorkerLeases;
    use crate::mail::worker::Post;

    /// Answers every call with one canned tool result and records the bodies.
    struct Canned {
        sent: RefCell<Vec<Value>>,
        reply: (u16, String),
    }

    fn canned(status: u16, text: &str, is_error: bool) -> Canned {
        let body = json!({"jsonrpc": "2.0", "id": 1, "result": {
            "content": [{"type": "text", "text": text}], "isError": is_error}});
        Canned {
            sent: RefCell::new(Vec::new()),
            reply: (status, body.to_string()),
        }
    }

    impl Post for &Canned {
        fn post(&self, body: &str) -> Result<(u16, String), String> {
            self.sent
                .borrow_mut()
                .push(serde_json::from_str(body).unwrap());
            Ok(self.reply.clone())
        }
    }

    const ITEM: &str = "o/r#4";

    #[test]
    fn acquire_calls_task_claim_with_lane_ttl_and_actor() {
        let c = canned(
            200,
            "claimed o/r#4 as node-a (lease l1-1, lane build, until 2026-11-24T16:00:00Z)",
            false,
        );
        let w = WorkerLeases::new(&c, Some("playpilot".into()));
        match w.acquire(ITEM, "build", 6).unwrap() {
            Acquired::Granted(g) => {
                assert_eq!((g.holder.as_str(), g.lease.as_str()), ("node-a", "l1-1"))
            }
            o => panic!("{o:?}"),
        }
        let sent = c.sent.borrow();
        assert_eq!(sent[0]["method"], "tools/call");
        assert_eq!(sent[0]["params"]["name"], "task_claim");
        assert_eq!(
            sent[0]["params"]["arguments"],
            json!({"item": ITEM, "lane": "build", "stale_hours": 6, "as": "playpilot"})
        );
    }

    #[test]
    fn a_held_item_is_a_refusal_not_an_error() {
        let c = canned(
            200,
            "not claimed o/r#4: the item has an open lease held by node-b",
            true,
        );
        let w = WorkerLeases::new(&c, None);
        assert_eq!(
            w.acquire(ITEM, "default", 6).unwrap(),
            Acquired::Refused("the item has an open lease held by node-b".into())
        );
        assert!(
            c.sent.borrow()[0]["params"]["arguments"]
                .get("as")
                .is_none()
        );
    }

    #[test]
    fn transport_failures_are_errors() {
        let c = canned(401, "", false);
        assert!(
            WorkerLeases::new(&c, None)
                .acquire(ITEM, "default", 6)
                .is_err()
        );
        let c = canned(200, "claimed but in a shape nobody knows", false);
        assert!(
            WorkerLeases::new(&c, None)
                .acquire(ITEM, "default", 6)
                .is_err()
        );
    }

    #[test]
    fn release_calls_task_release_and_tolerates_a_gone_lease() {
        let c = canned(200, "released o/r#4", false);
        WorkerLeases::new(&c, None).release(ITEM).unwrap();
        assert_eq!(c.sent.borrow()[0]["params"]["name"], "task_release");
        assert_eq!(c.sent.borrow()[0]["params"]["arguments"]["item"], ITEM);
        let gone = canned(200, "no open lease with that id", true);
        WorkerLeases::new(&gone, None).release(ITEM).unwrap();
        let theirs = canned(200, "the lease belongs to another holder", true);
        assert!(WorkerLeases::new(&theirs, None).release(ITEM).is_err());
    }
}

#[test]
fn backends_parse() {
    use super::cli::{Backend, parse_backend};
    assert_eq!(parse_backend("worker").unwrap(), Backend::Worker);
    assert_eq!(parse_backend("github").unwrap(), Backend::GitHub);
    assert!(parse_backend("kv").is_err());
}
