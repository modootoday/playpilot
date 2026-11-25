//! Remote MCP over HTTP POST (JSON-RPC 2.0). `protocol` turns a request into
//! a reply or a `tools::Command`; the runtime executes the command against the
//! Durable Objects and wraps the text with `protocol::tool_result`.

pub mod events;
pub mod protocol;
pub mod tools;
