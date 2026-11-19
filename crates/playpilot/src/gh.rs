//! Infrastructure: GitHub REST calls through the `gh` CLI (its login, its proxy).

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

/// Runs `gh api <args>`, optionally feeding `stdin`, and parses the JSON reply.
/// An empty reply (204) is `null`.
pub fn api(args: &[&str], stdin: Option<&str>) -> Result<Value, String> {
    let mut cmd = Command::new("gh");
    cmd.arg("api").args(args);
    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("could not run gh: {e}"))?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(input.as_bytes())
            .map_err(|e| format!("could not write to gh: {e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("gh failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    if out.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("gh returned non-JSON: {e}"))
}
