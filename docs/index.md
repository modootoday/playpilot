---
title: Claude Code agents under one set of rules
description: Run Claude Code agents across your devices, cloud sessions and containers under one set of rules.
---
# playpilot

playpilot runs Claude Code agents on your own machines, in Claude cloud
sessions and in containers, under one set of rules: work arrives through pull
requests, every runtime shares the same guard, and the history on your default
branch stays clean.

## Install

```
/plugin marketplace add modootoday/playpilot
/plugin install playpilot@playpilot
/playpilot:install
```

The plugin brings the guard hook, the fleet roles and the `fleet-claim` skill.
See [Install a node](guides/install.md) for a home device and
[Architecture](architecture.md) for how the pieces fit.

## What it enforces

- Changes reach the default branch only through pull requests, with auto-merge
  once the required checks pass.
- No force pushes outside your own agent branch, no tags or releases from
  agents, no direct merges or approvals.
- Conventional commit subjects, no merge commits in topic branches, and the
  owner's noreply identity on every commit ([commit history](https://github.com/modootoday/playpilot/blob/main/.agents/rules/commit-history.md)).

## Reference

- [Architecture](architecture.md)
- [Configuration, interfaces and extensions](config-and-extensions.md)
- [Operations domain](domain.md)
