use super::*;

fn item(n: u64) -> ItemRef {
    ItemRef {
        repo: "o/r".into(),
        number: n,
    }
}

fn open() -> Limits {
    Limits::default()
}

#[test]
fn one_holder_per_item() {
    let mut b = LeaseBook::default();
    let l = b
        .acquire("build", &item(1), "node-a", 600, open(), 100)
        .unwrap();
    assert_eq!(l.expires_at, 700);
    let err = b
        .acquire("build", &item(1), "node-b", 600, open(), 200)
        .unwrap_err();
    assert_eq!(
        err,
        LeaseError::Held {
            holder: "node-a".into()
        }
    );
    assert_eq!(err.status().0, 409);
    // Repository case does not open a second lease.
    let other = ItemRef {
        repo: "O/R".into(),
        number: 1,
    };
    assert!(
        b.acquire("build", &other, "node-b", 600, open(), 200)
            .is_err()
    );
}

#[test]
fn same_holder_retry_renews() {
    let mut b = LeaseBook::default();
    let first = b
        .acquire("x", &item(1), "node-a", 600, open(), 100)
        .unwrap();
    let again = b
        .acquire("x", &item(1), "node-a", 600, open(), 400)
        .unwrap();
    assert_eq!(first.id, again.id);
    assert_eq!(again.expires_at, 1_000);
    assert_eq!(b.leases.len(), 1);
}

#[test]
fn expiry_frees_the_item() {
    let mut b = LeaseBook::default();
    b.acquire("x", &item(1), "node-a", 600, open(), 100)
        .unwrap();
    assert_eq!(b.expire(699).len(), 0);
    let gone = b.expire(700);
    assert_eq!(gone.len(), 1);
    let l = b
        .acquire("x", &item(1), "node-b", 600, open(), 701)
        .unwrap();
    assert_eq!(l.holder, "node-b");
}

#[test]
fn acquire_expires_stale_leases_first() {
    let mut b = LeaseBook::default();
    b.acquire("x", &item(1), "node-a", 60, open(), 0).unwrap();
    assert!(b.acquire("x", &item(1), "node-b", 60, open(), 61).is_ok());
}

#[test]
fn renew_and_release_need_the_holder() {
    let mut b = LeaseBook::default();
    let l = b
        .acquire("x", &item(1), "node-a", 600, open(), 100)
        .unwrap();
    assert_eq!(b.renew(&l.id, "node-b", 200), Err(LeaseError::NotHolder));
    assert_eq!(b.renew(&l.id, "node-a", 500).unwrap().expires_at, 1_100);
    assert_eq!(b.release(&l.id, "node-b", 600), Err(LeaseError::NotHolder));
    assert_eq!(b.release("nope", "node-a", 600), Err(LeaseError::NotFound));
    assert!(b.release(&l.id, "node-a", 600).is_ok());
    assert_eq!(b.release(&l.id, "node-a", 600), Err(LeaseError::NotFound));
}

#[test]
fn expired_lease_cannot_be_renewed() {
    let mut b = LeaseBook::default();
    let l = b.acquire("x", &item(1), "node-a", 60, open(), 0).unwrap();
    assert_eq!(b.renew(&l.id, "node-a", 60), Err(LeaseError::NotFound));
}

#[test]
fn release_by_item() {
    let mut b = LeaseBook::default();
    b.acquire("x", &item(3), "node-a", 600, open(), 0).unwrap();
    assert_eq!(
        b.release_item(&item(3), "node-b", 1),
        Err(LeaseError::NotHolder)
    );
    assert!(b.release_item(&item(3), "node-a", 1).is_ok());
}

#[test]
fn paused_lane_or_node_refuses_new_leases() {
    let mut b = LeaseBook::default();
    let paused = Ops {
        paused: Some(true),
        capacity: None,
    };
    let lane = Limits {
        lane: paused,
        device: Ops::default(),
    };
    let err = b
        .acquire("x", &item(1), "node-a", 600, lane, 0)
        .unwrap_err();
    assert_eq!(err, LeaseError::Paused("lane x".into()));
    let dev = Limits {
        lane: Ops::default(),
        device: paused,
    };
    assert!(matches!(
        b.acquire("x", &item(1), "node-a", 600, dev, 0),
        Err(LeaseError::Paused(_))
    ));
}

#[test]
fn capacity_counts_open_leases() {
    let mut b = LeaseBook::default();
    let one = Ops {
        paused: None,
        capacity: Some(1),
    };
    let dev = Limits {
        lane: Ops::default(),
        device: one,
    };
    b.acquire("x", &item(1), "node-a", 600, dev, 0).unwrap();
    assert_eq!(
        b.acquire("y", &item(2), "node-a", 600, dev, 0),
        Err(LeaseError::AtCapacity("node node-a".into()))
    );
    let lane = Limits {
        lane: one,
        device: Ops::default(),
    };
    assert!(matches!(
        b.acquire("x", &item(2), "node-b", 600, lane, 0),
        Err(LeaseError::AtCapacity(_))
    ));
    let zero = Limits {
        lane: Ops {
            paused: None,
            capacity: Some(0),
        },
        device: Ops::default(),
    };
    assert!(b.acquire("z", &item(9), "node-b", 600, zero, 0).is_err());
}

#[test]
fn validates_input() {
    let mut b = LeaseBook::default();
    assert!(matches!(
        b.acquire("x", &item(1), "Node A", 600, open(), 0),
        Err(LeaseError::Invalid(_))
    ));
    assert!(b.acquire("", &item(1), "node-a", 600, open(), 0).is_err());
    assert!(b.acquire("x", &item(0), "node-a", 600, open(), 0).is_err());
    assert_eq!(clamp_ttl(None), Ok(DEFAULT_TTL));
    assert!(clamp_ttl(Some(10)).is_err());
    assert!(clamp_ttl(Some(MAX_TTL + 1)).is_err());
}

#[test]
fn apply_renews_only_own_leases_and_counts_lanes() {
    let mut b = LeaseBook::default();
    let a = b.acquire("x", &item(1), "node-a", 600, open(), 0).unwrap();
    let c = b.acquire("x", &item(2), "node-b", 600, open(), 0).unwrap();
    let reply = apply(
        &mut b,
        LeaseCmd::Renew {
            ids: vec![a.id.clone(), c.id.clone()],
            holder: "node-a".into(),
        },
        300,
    );
    assert_eq!(reply, LeaseReply::Renewed(vec![a.id]));
    match apply(&mut b, LeaseCmd::PerLane, 700) {
        LeaseReply::PerLane(m) => assert_eq!(m.get("x"), Some(&1)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn api_shape() {
    let mut b = LeaseBook::default();
    let l = b.acquire("x", &item(1), "node-a", 600, open(), 0).unwrap();
    let v = l.to_api();
    assert_eq!(v["createdAt"], "1970-01-01T00:00:00Z");
    assert_eq!(v["expiresAt"], "1970-01-01T00:10:00Z");
    assert_eq!(v["item"]["repo"], "o/r");
}
