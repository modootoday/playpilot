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

## Install (Claude Code plugin)

```
/plugin marketplace add modootoday/playpilot
/plugin install playpilot@playpilot
/playpilot:install
```

The plugin brings the guard hook, the fleet roles (`playpilot:implementer`,
`playpilot:reviewer`, `playpilot:heavy-builder`, `playpilot:windows-validator`)
and the `fleet-claim` skill. Cloud sessions do not load marketplace plugins;
they get the same guard from the `.claude/settings.json` that
`playpilot render` writes into each repository.

playpilot's guard covers git and release boundaries. Do not enable another
plugin that guards the same commands in the same session; two hooks on one
command make refusals harder to read.
