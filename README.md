# playpilot

Starter for running Claude Code agents across home Windows/WSL devices, Claude
cloud sessions and Cloudflare containers under one set of rules.

- `AGENTS.md`, `.agents/`: governance shared by every runtime
- `.claude/settings.json`: repo permission baseline and the `playpilot guard` hook
- `profiles/node.settings.json`: auto mode profile for nodes (`--settings`)
- `crates/playpilot`: PreToolUse hook (Rust) enforcing git/release boundaries
- `devices/`: one manifest per physical device
- `runtimes/{wsl,windows,claude-cloud,cf-container}`: bootstrap per runtime
- `docs/architecture.md`: the design

Quick start (WSL): clone to `~/fleet/playpilot`, run
`runtimes/wsl/bootstrap.sh`, then follow its printed owner steps.
