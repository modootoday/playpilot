//! Port: where lane items and their comments live.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub body: String,
    /// RFC 3339 UTC.
    pub created_at: String,
}

/// An item is addressed as `owner/repo#number`.
pub trait Tracker {
    /// Comments on the item, oldest first.
    fn comments(&self, item: &str) -> Result<Vec<Comment>, String>;
    fn comment(&self, item: &str, body: &str) -> Result<(), String>;
    fn add_label(&self, item: &str, label: &str) -> Result<(), String>;
    /// Removing a label that is not there is not an error.
    fn remove_label(&self, item: &str, label: &str) -> Result<(), String>;
    /// Open issues and pull requests in `repo` (`owner/repo`) with `label`.
    fn labelled(&self, repo: &str, label: &str) -> Result<Vec<String>, String>;
}

/// Splits `owner/repo#12` into (`owner/repo`, 12).
pub fn parse_item(item: &str) -> Result<(String, u64), String> {
    let (repo, n) = item
        .split_once('#')
        .ok_or_else(|| format!("`{item}` is not owner/repo#number"))?;
    let ok_repo = repo.split('/').count() == 2
        && repo.split('/').all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        });
    let n: u64 = n
        .parse()
        .map_err(|_| format!("`{item}` is not owner/repo#number"))?;
    if !ok_repo || n == 0 {
        return Err(format!("`{item}` is not owner/repo#number"));
    }
    Ok((repo.to_string(), n))
}
