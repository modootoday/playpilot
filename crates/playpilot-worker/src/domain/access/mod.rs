//! Cloudflare Access assertion checks for `/v1/*`.
//!
//! The Worker accepts a caller only when `Cf-Access-Jwt-Assertion` is an
//! RS256 token signed by one of the team's keys, issued by the team, meant
//! for this application and currently valid. The identity key is the
//! token's `common_name` (the service token client id), never a header.
//! Signature verification itself runs in `infra` (WebCrypto).

pub mod base64url;
pub mod jwks;
pub mod jwt;
pub mod policy;

#[cfg(test)]
mod tests;

/// Why an assertion was refused. Logged, never sent to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    Malformed(&'static str),
    Algorithm,
    UnknownKey,
    Signature,
    Issuer,
    Audience,
    Expired,
    NotYetValid,
}

impl std::fmt::Display for Reject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reject::Malformed(why) => write!(f, "malformed assertion: {why}"),
            Reject::Algorithm => f.write_str("assertion algorithm is not RS256"),
            Reject::UnknownKey => f.write_str("assertion key id is not a team key"),
            Reject::Signature => f.write_str("assertion signature does not verify"),
            Reject::Issuer => f.write_str("assertion issuer is not the team"),
            Reject::Audience => f.write_str("assertion audience is not this application"),
            Reject::Expired => f.write_str("assertion has expired"),
            Reject::NotYetValid => f.write_str("assertion is not valid yet"),
        }
    }
}

/// The identity key of a verified assertion: the service token client id.
/// Tokens from interactive logins carry none and get no identity.
pub fn service_token_id(claims: &jwt::Claims) -> Option<&str> {
    claims.common_name.as_deref()
}
