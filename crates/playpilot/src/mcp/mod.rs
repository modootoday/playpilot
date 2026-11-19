//! `playpilot mcp`: a Model Context Protocol server over stdio.
//!
//! Layers: `protocol` (JSON-RPC 2.0 framing and the MCP lifecycle) and
//! `tools` (fleet use cases exposed as tools). The session's actor and
//! mailbox repository come from the environment, never from tool arguments,
//! so a tool call cannot speak for another actor.

pub mod protocol;
pub mod tools;

use std::io::{BufRead, Write};
use std::process::ExitCode;

use crate::mail::github::GitHub;

pub fn main() -> ExitCode {
    let repo = std::env::var("PLAYPILOT_MAILBOX_REPO").ok();
    let actor = std::env::var("PLAYPILOT_ACTOR").ok();
    let transport = repo.clone().map(|repo| GitHub { repo });
    let tracker = crate::claim::github::GitHubTracker;
    let tools = tools::MailTools {
        transport: transport.as_ref().map(|t| t as _),
        actor,
        repo,
        tracker: Some(&tracker),
        claim_id: crate::claim::cli::env_id(),
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = protocol::handle(&tools, &line)
            && writeln!(stdout, "{reply}")
                .and_then(|()| stdout.flush())
                .is_err()
        {
            break;
        }
    }
    ExitCode::SUCCESS
}
