# Permission layers

Each layer has one job. Lower layers never loosen a higher one.

| Layer | File | Applies in | Job |
| --- | --- | --- | --- |
| Repo baseline | `.claude/settings.json` (committed) | Every runtime, including cloud sessions | allow/deny rules, the `playpilot guard` PreToolUse hook |
| Node profile | `profiles/node.settings.json`, passed with `--settings` | WSL, Windows, Cloudflare container | `defaultMode: auto`, `autoMode` environment and deny rules, bypass mode disabled |
| Guard | `crates/playpilot` | Wherever the hook runs | Hard refusals that prose rules cannot guarantee (force push, protected branch, tags, releases, merges, secrets, rulesets, trailers) |
| Owner | GitHub rulesets, required checks | GitHub | The final gate; agents cannot change it |

Why the split: Claude Code ignores `autoMode` and `defaultMode: auto` in
project settings, and cloud sessions do not read user settings. So the repo
file carries only what every runtime can honour, and the node profile carries
the rest.
