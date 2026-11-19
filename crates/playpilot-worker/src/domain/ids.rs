//! Value types shared by the API: opaque node/actor names, item references, revs.

use serde::{Deserialize, Serialize};

/// Node ids and actor names: lower-case words joined by single hyphens
/// (`^[a-z0-9]+(-[a-z0-9]+)*$`), at most 40 characters.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// A full 40-character lower-case hex commit SHA.
pub fn valid_rev(rev: &str) -> bool {
    rev.len() == 40
        && rev
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Lane names: as node names, plus `.` and `_`, at most 64 characters.
pub fn valid_lane(lane: &str) -> bool {
    !lane.is_empty()
        && lane.len() <= 64
        && lane
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"-_.".contains(&c))
}

/// An issue or pull request, `owner/repo#number`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemRef {
    pub repo: String,
    pub number: u64,
}

impl ItemRef {
    pub fn validate(&self) -> Result<(), String> {
        let parts: Vec<&str> = self.repo.split('/').collect();
        let ok = parts.len() == 2
            && self.repo.len() <= 140
            && parts.iter().all(|p| {
                !p.is_empty()
                    && p.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            });
        if !ok || self.number == 0 {
            return Err(format!(
                "`{}#{}` is not owner/repo#number",
                self.repo, self.number
            ));
        }
        Ok(())
    }

    /// Parses `owner/repo#12`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let bad = || format!("`{text}` is not owner/repo#number");
        let (repo, n) = text.split_once('#').ok_or_else(bad)?;
        let item = Self {
            repo: repo.to_string(),
            number: n.parse().map_err(|_| bad())?,
        };
        item.validate().map_err(|_| bad())?;
        Ok(item)
    }

    /// The lease key; repository names are case-insensitive on GitHub.
    pub fn key(&self) -> String {
        format!("{}#{}", self.repo.to_ascii_lowercase(), self.number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_name("node-a"));
        assert!(valid_name("n1"));
        for bad in ["", "-a", "a-", "a--b", "Node", "a_b", &"a".repeat(41)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn revs() {
        assert!(valid_rev(&"a1".repeat(20)));
        assert!(!valid_rev(&"A1".repeat(20)));
        assert!(!valid_rev("abc"));
    }

    #[test]
    fn items() {
        let item = ItemRef::parse("Owner/Repo.rs#12").unwrap();
        assert_eq!(item.number, 12);
        assert_eq!(item.key(), "owner/repo.rs#12");
        for bad in [
            "owner/repo",
            "owner#1",
            "owner/repo#0",
            "a/b/c#1",
            "o/r#x",
            "o w/r#1",
        ] {
            assert!(ItemRef::parse(bad).is_err(), "{bad}");
        }
    }
}
