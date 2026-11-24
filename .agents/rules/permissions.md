# Permission layers

Each layer has one job. Lower layers never loosen a higher one. The layers
implement the autonomy levels in `governance.md` (A0 auto, A1 measured,
A2 owner).

| Layer | File | Applies in | Job |
| --- | --- | --- | --- |
| Owner | GitHub rulesets, required checks, Cloudflare Access | GitHub, Cloudflare | The final gate; agents cannot change it |
| Guard | `crates/playpilot` (`playpilot guard`) | Wherever a PreToolUse hook runs it | Hard refusals prose cannot guarantee: protected-branch and tag pushes, force push other than an own-branch lease, merges without `--auto`, approvals, releases, secrets, rulesets, sensitive API writes, trailers |
| Repo baseline | `.claude/settings.json` (committed; rendered for projects) | Every runtime that opens the project as its root | `allow` for A0 routine commands, `ask` for deploys and Worker secrets, `deny` for A2 commands and credential files; the guard hook |
| User layer (cloud) | `~/.claude/settings.json`, written by the environment setup script | Cloud sessions | `autoMode` context, allow and hard_deny lines; a user-level guard hook, because a session that attaches a second repository moves its project root and stops loading the project's hooks |
| Node profile | `profiles/node.settings.json`, passed with `--settings` | WSL, Windows, Cloudflare container nodes | `defaultMode: auto`, the same `autoMode` lines as the user layer, bypass mode disabled |

Why the split: Claude Code reads `autoMode` and `defaultMode: auto` only from
user or managed settings, never from project settings, and cloud sessions do
not read a user's local settings. So the repository file carries only rules
every runtime honours, and the user layer and node profile carry the rest.

Rules for changing a layer:
- Adding to `deny`, `hard_deny` or the guard (narrowing) is A0.
- Adding to `allow`, `ask` or an auto-mode allow line (widening) is A1.
- Changing the owner layer is A2.
- `allow` never lists a broad pattern (`Bash(*)`, `Bash(gh api *)`): auto mode
  drops broad allows, and a broad allow hides the guard's intent.
