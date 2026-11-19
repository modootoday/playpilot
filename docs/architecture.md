---
---
# Architecture

One repository configures every place an agent runs: two home devices, Claude
cloud sessions, and (optionally) Cloudflare containers. The same governance
applies everywhere; only the runtime layer differs.

```
                    GitHub (only shared state)
          issues/PRs · lane labels · claims · rulesets · checks
             ▲                ▲                 ▲              ▲
             │ gh             │ gh              │ connector    │ token injected
   ┌─────────┴──────┐ ┌───────┴────────┐ ┌──────┴───────┐ ┌────┴──────────────┐
   │ Workstation    │ │ Build node     │ │ Claude cloud │ │ CF container      │
   │ Windows + WSL  │ │ WSL (+Windows) │ │ sessions /   │ │ (optional, paid   │
   │ RC server      │ │ RC server      │ │ scheduled    │ │ mode) claude -p   │
   │ windows/gui    │ │ heavy-build    │ │ tasks        │ │ dispatched by a   │
   │ lanes          │ │ lane           │ │              │ │ Worker            │
   └────────────────┘ └────────────────┘ └──────────────┘ └───────────────────┘
         ▲  steer/observe from phone or claude.ai (Remote Control)
```

## Layers

| Layer | Same everywhere? | Content |
| --- | --- | --- |
| Governance | yes | `AGENTS.md`, `.agents/rules`, plugin agents and skills (`plugin/`) |
| Repo baseline | yes | `.claude/settings.json`: allow/deny rules and the `playpilot guard` hook |
| Guard | yes | `crates/playpilot`, installed by every runtime bootstrap |
| Node profile | not in cloud sessions | `profiles/node.settings.json` via `--settings`: auto mode and its rules |
| Runtime | no | `runtimes/<name>/`: install, credentials, process supervision |

## Runtimes

| Runtime | Start | Auth | Remote Control | Supervision |
| --- | --- | --- | --- | --- |
| `wsl` | `systemctl --user enable --now claude-rc@<dir>` | `claude auth login` (subscription, full scope) | yes | systemd `Restart=always` |
| `windows` | Task Scheduler → `rc-server.ps1` | `claude auth login` | yes | restart loop |
| `claude-cloud` | scheduled task / session | claude.ai account (always subscription) | n/a | Anthropic |
| `cf-container` | Worker → container `fleet-entrypoint` | paid: API key injected by the outbound handler; free: owner's `setup-token`, model requests only | no | container lifecycle |

## Free and paid modes

- Free: home devices on the owner's subscription login, plus Claude cloud
  scheduled tasks. Usage is the owner's ordinary individual use; no third-party
  harness touches subscription credentials.
- Paid: add an Anthropic API key for headless jobs (`claude -p` in the CF
  container or on a device), optionally through Cloudflare AI Gateway. Remote
  Control sessions still use the subscription login.

## Owner-only steps

`claude auth login` on each device (renew before expiry), `gh auth login`,
applying rulesets and labels from files, entering secret values, releases.
