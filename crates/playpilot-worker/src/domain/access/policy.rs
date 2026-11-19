//! What a valid assertion must say: issued by the team, for this application,
//! and inside its validity window (60 s clock skew either way).

use super::Reject;
use super::jwt::Claims;

pub const SKEW: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    team_domain: String,
    audience: String,
}

/// A bare host name: dot-separated labels of letters, digits and `-`.
fn valid_host(h: &str) -> bool {
    h.len() <= 253
        && h.contains('.')
        && h.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
}

impl Policy {
    /// The team domain is a bare host name (no scheme or path); the audience
    /// is the Access application's AUD tag. Either missing means no policy.
    pub fn new(team_domain: Option<&str>, audience: Option<&str>) -> Result<Self, String> {
        let domain = team_domain
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .ok_or("the Access team domain is not set")?;
        let audience = audience
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .ok_or("the Access audience is not set")?;
        if !valid_host(domain) {
            return Err("the Access team domain must be a bare host name".into());
        }
        Ok(Self {
            team_domain: domain.to_ascii_lowercase(),
            audience: audience.to_string(),
        })
    }

    pub fn issuer(&self) -> String {
        format!("https://{}", self.team_domain)
    }

    pub fn certs_url(&self) -> String {
        format!("https://{}/cdn-cgi/access/certs", self.team_domain)
    }

    pub fn check(&self, claims: &Claims, now: u64) -> Result<(), Reject> {
        if claims.iss != self.issuer() {
            return Err(Reject::Issuer);
        }
        if !claims.aud.contains(&self.audience) {
            return Err(Reject::Audience);
        }
        if claims.exp.saturating_add(SKEW) <= now {
            return Err(Reject::Expired);
        }
        if claims.nbf.is_some_and(|nbf| nbf > now.saturating_add(SKEW)) {
            return Err(Reject::NotYetValid);
        }
        Ok(())
    }
}
