//! Phase B Cloudflare Worker (docs/config-and-extensions.md, section 5).
//!
//! `domain` is plain Rust and runs on the host; `infra` binds it to the
//! Workers runtime and Durable Objects and only builds for `wasm32`.

pub mod domain;

#[cfg(target_arch = "wasm32")]
mod infra;
