//! Lane-item leases: one holder per item, a TTL, renew, release and expiry.
//! `LeaseObject` loads a `LeaseBook`, applies one `LeaseCmd` and saves it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ids::{ItemRef, valid_lane, valid_name};
use super::ops::Ops;
use super::time::rfc3339;

pub const DEFAULT_TTL: u64 = 6 * 3_600;
pub const MIN_TTL: u64 = 60;
pub const MAX_TTL: u64 = 24 * 3_600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub id: String,
    pub lane: String,
    pub item: ItemRef,
    pub holder: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub ttl: u64,
}

impl Lease {
    /// The OpenAPI `Lease` representation.
    pub fn to_api(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "lane": self.lane,
            "item": self.item,
            "holder": self.holder,
            "createdAt": rfc3339(self.created_at),
            "expiresAt": rfc3339(self.expires_at),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeaseError {
    Invalid(String),
    Held { holder: String },
    Paused(String),
    AtCapacity(String),
    NotFound,
    NotHolder,
}

impl LeaseError {
    /// HTTP status and title for problem details.
    pub fn status(&self) -> (u16, &'static str) {
        match self {
            Self::Invalid(_) => (400, "Bad request"),
            Self::Held { .. } => (409, "Item already leased"),
            Self::Paused(_) => (409, "Paused"),
            Self::AtCapacity(_) => (409, "At capacity"),
            Self::NotFound => (404, "No such lease"),
            Self::NotHolder => (403, "Lease belongs to another holder"),
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::Invalid(m) => m.clone(),
            Self::Held { holder } => format!("the item has an open lease held by {holder}"),
            Self::Paused(what) => format!("{what} is paused"),
            Self::AtCapacity(what) => format!("{what} is at capacity"),
            Self::NotFound => "no open lease with that id".into(),
            Self::NotHolder => "the lease belongs to another holder".into(),
        }
    }
}

/// Effective (already narrowed) switches for one acquire.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Limits {
    pub lane: Ops,
    pub device: Ops,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseBook {
    /// Open leases keyed by `ItemRef::key`.
    pub leases: BTreeMap<String, Lease>,
    pub seq: u64,
}

pub fn clamp_ttl(ttl: Option<u64>) -> Result<u64, LeaseError> {
    let ttl = ttl.unwrap_or(DEFAULT_TTL);
    if !(MIN_TTL..=MAX_TTL).contains(&ttl) {
        return Err(LeaseError::Invalid(format!(
            "ttl must be between {MIN_TTL}s and {MAX_TTL}s"
        )));
    }
    Ok(ttl)
}

impl LeaseBook {
    /// Drops leases whose TTL has passed; returns them.
    pub fn expire(&mut self, now: u64) -> Vec<Lease> {
        let gone: Vec<String> = self
            .leases
            .iter()
            .filter(|(_, l)| l.expires_at <= now)
            .map(|(k, _)| k.clone())
            .collect();
        gone.iter().filter_map(|k| self.leases.remove(k)).collect()
    }

    fn find_id(&self, id: &str) -> Option<String> {
        self.leases
            .iter()
            .find(|(_, l)| l.id == id)
            .map(|(k, _)| k.clone())
    }

    pub fn acquire(
        &mut self,
        lane: &str,
        item: &ItemRef,
        holder: &str,
        ttl: u64,
        limits: Limits,
        now: u64,
    ) -> Result<Lease, LeaseError> {
        if !valid_lane(lane) {
            return Err(LeaseError::Invalid(format!("`{lane}` is not a lane name")));
        }
        if !valid_name(holder) {
            return Err(LeaseError::Invalid(format!("`{holder}` is not a node id")));
        }
        item.validate().map_err(LeaseError::Invalid)?;
        self.expire(now);
        let key = item.key();
        if let Some(open) = self.leases.get_mut(&key) {
            if open.holder != holder {
                return Err(LeaseError::Held {
                    holder: open.holder.clone(),
                });
            }
            // A retried claim by the same holder renews the lease it already has.
            open.ttl = ttl;
            open.expires_at = now + ttl;
            return Ok(open.clone());
        }
        if limits.lane.is_paused() {
            return Err(LeaseError::Paused(format!("lane {lane}")));
        }
        if limits.device.is_paused() {
            return Err(LeaseError::Paused(format!("node {holder}")));
        }
        let in_lane = self.leases.values().filter(|l| l.lane == lane).count() as u64;
        if limits.lane.capacity.is_some_and(|c| in_lane >= c) {
            return Err(LeaseError::AtCapacity(format!("lane {lane}")));
        }
        let held = self.leases.values().filter(|l| l.holder == holder).count() as u64;
        if limits.device.capacity.is_some_and(|c| held >= c) {
            return Err(LeaseError::AtCapacity(format!("node {holder}")));
        }
        self.seq += 1;
        let lease = Lease {
            id: format!("l{:x}-{}", now, self.seq),
            lane: lane.to_string(),
            item: item.clone(),
            holder: holder.to_string(),
            created_at: now,
            expires_at: now + ttl,
            ttl,
        };
        self.leases.insert(key, lease.clone());
        Ok(lease)
    }

    /// Extends a lease by its own TTL; only its holder may.
    pub fn renew(&mut self, id: &str, holder: &str, now: u64) -> Result<Lease, LeaseError> {
        self.expire(now);
        let key = self.find_id(id).ok_or(LeaseError::NotFound)?;
        let lease = self.leases.get_mut(&key).ok_or(LeaseError::NotFound)?;
        if lease.holder != holder {
            return Err(LeaseError::NotHolder);
        }
        lease.expires_at = now + lease.ttl;
        Ok(lease.clone())
    }

    pub fn release(&mut self, id: &str, holder: &str, now: u64) -> Result<Lease, LeaseError> {
        self.expire(now);
        let key = self.find_id(id).ok_or(LeaseError::NotFound)?;
        self.release_key(&key, holder)
    }

    pub fn release_item(
        &mut self,
        item: &ItemRef,
        holder: &str,
        now: u64,
    ) -> Result<Lease, LeaseError> {
        self.expire(now);
        self.release_key(&item.key(), holder)
    }

    fn release_key(&mut self, key: &str, holder: &str) -> Result<Lease, LeaseError> {
        match self.leases.get(key) {
            None => Err(LeaseError::NotFound),
            Some(l) if l.holder != holder => Err(LeaseError::NotHolder),
            Some(_) => self.leases.remove(key).ok_or(LeaseError::NotFound),
        }
    }

    /// Open leases per lane.
    pub fn per_lane(&self) -> BTreeMap<String, u64> {
        let mut out = BTreeMap::new();
        for l in self.leases.values() {
            *out.entry(l.lane.clone()).or_insert(0) += 1;
        }
        out
    }
}

/// One operation sent from the router to `LeaseObject`. Holders here are
/// already the caller's authenticated claim id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LeaseCmd {
    Acquire {
        lane: String,
        item: ItemRef,
        holder: String,
        ttl: u64,
        limits: Limits,
    },
    Release {
        id: String,
        holder: String,
    },
    ReleaseItem {
        item: ItemRef,
        holder: String,
    },
    /// Heartbeat renewal; ids the holder no longer has are skipped.
    Renew {
        ids: Vec<String>,
        holder: String,
    },
    PerLane,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeaseReply {
    Lease(Lease),
    Renewed(Vec<String>),
    PerLane(BTreeMap<String, u64>),
    Error(LeaseError),
}

/// Applies `cmd`; the caller persists `book` afterwards.
pub fn apply(book: &mut LeaseBook, cmd: LeaseCmd, now: u64) -> LeaseReply {
    let result = match cmd {
        LeaseCmd::Acquire {
            lane,
            item,
            holder,
            ttl,
            limits,
        } => book.acquire(&lane, &item, &holder, ttl, limits, now),
        LeaseCmd::Release { id, holder } => book.release(&id, &holder, now),
        LeaseCmd::ReleaseItem { item, holder } => book.release_item(&item, &holder, now),
        LeaseCmd::Renew { ids, holder } => {
            let ok = ids
                .iter()
                .filter(|id| book.renew(id, &holder, now).is_ok())
                .cloned()
                .collect();
            return LeaseReply::Renewed(ok);
        }
        LeaseCmd::PerLane => {
            book.expire(now);
            return LeaseReply::PerLane(book.per_lane());
        }
    };
    match result {
        Ok(lease) => LeaseReply::Lease(lease),
        Err(e) => LeaseReply::Error(e),
    }
}

#[cfg(test)]
#[path = "lease_tests.rs"]
mod tests;
