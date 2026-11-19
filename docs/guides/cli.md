---
---
# CLI reference

`playpilot` is one binary. Every subcommand below exists on `main` today;
[Configuration, interfaces and extensions](../config-and-extensions.md#7-cli-surface)
lists the ones that are still planned.

`playpilot --version` prints the version. An unknown or missing subcommand
prints the usage summary and exits `64`.

## Exit codes

Every subcommand uses the same small set:

| Code | Meaning |
| --- | --- |
| `0` | success, or the check passed |
| `1` | the check found problems, the file is stale, the item is held by someone else, or the operation failed |
| `2` | `guard`: the tool call is blocked. `history-check`, `surface-check`: git could not read the range |
| `64` | usage error (unknown argument, missing required option, `--help`) |

## Environment variables

| Variable | Read by | Meaning |
| --- | --- | --- |
| `PLAYPILOT_PROTECTED_BRANCHES` | `guard` | comma-separated branches nobody pushes to; default `main,master` |
| `PLAYPILOT_DEVICE` | `whoami` | device name when `--device` is not given |
| `PLAYPILOT_ACTOR` | `mail`, `mcp`, `claim`, `release` | this session's actor (mailbox name); the claim id when `PLAYPILOT_CLAIM_ID` is unset |
| `PLAYPILOT_CLAIM_ID` | `claim`, `release`, `mcp` | the id written on claims; use one that names no device |
| `PLAYPILOT_MAIL_URL` | `mail`, `mcp` | the Worker's `/mcp` URL; when set, mail goes through the Worker |
| `PLAYPILOT_MCP_TOKEN` | `mail`, `mcp` | bearer token for the Worker's `/mcp` |
| `PLAYPILOT_MAILBOX_REPO` | `mail`, `mcp` | `owner/name` of the mailbox repository (GitHub transport) |

`claim`, `release` and the GitHub mail transport call GitHub through the `gh`
CLI and its login. The Worker mail transport uses `curl`.

## guard

Claude Code PreToolUse hook. Reads the hook event JSON on stdin and checks
`Bash` and `PowerShell` commands: no force push outside the session's own
agent branch, no push to a protected branch, no tags or releases, no direct
merge or approval, explicit staging only, no attribution trailers other than
`@users.noreply.github.com` co-authors.

```
playpilot guard < event.json
```

Exit `0` allows the tool call; exit `2` blocks it and the reason on stderr is
shown to Claude. Input that is not JSON is blocked.

## history-check

The `History` required check ([commit history](https://github.com/modootoday/playpilot/blob/main/.agents/rules/commit-history.md)).

```
playpilot history-check --range <base>..<head> [--method squash|merge] [--title <PR title>]
```

With `--method squash` (the default) the pull request title must be a
Conventional Commits subject; with `merge` every commit subject must be. In
both cases the range may hold no merge commits, no `WIP`, `fixup!`, `squash!`,
`amend!` or empty commits, no forbidden trailers, and author and committer
must be noreply addresses. Exit `0` passes, `1` lists violations, `2` when
git cannot read the range.

## surface-check

The `Public surface` required check. Scans added lines, file names, commit
messages and optional extra text for deny terms, personal email addresses and
local user paths. Findings name the location and the term's index, never the
term.

```
playpilot surface-check --range <base>..<head> [--text-env NAME]... [--deny-env NAME]
```

`--text-env` adds the value of an environment variable (for example the pull
request title and body) to the scan; `--deny-env` names the variable that
holds the deny terms, one per line. Without terms only the generic scans run.
Exit `0` passes, `1` lists findings, `2` when git cannot read the range.

## validate

Checks a fleet configuration directory: schema and semantics (references
resolve, every lane has a capable device, profiles only narrow, no secret or
private host values).

```
playpilot validate <fleet-dir>
```

Exit `0` valid, `1` lists problems.

## render

Writes files from the fleet configuration repository (`--config`, default the
current directory, holding `fleet/` and `playpilot.lock`).

```
playpilot render settings --project <name> [--config <dir>] [--repo <dir>] [--check]
playpilot render prompt   --session <name> [--config <dir>]
playpilot render node     --device <name> [--workspace <dir>] [--config <dir>] [--out <dir> [--check]]
```

- `settings`: writes the guard hook into `<repo>/.claude/settings.json`,
  pinned to the rev in `playpilot.lock`, keeping other settings and hooks.
- `prompt`: prints a session's scheduled-task prompt.
- `node`: prints, or with `--out` writes, a device's service files
  (`claude-rc@.service` for `wsl`, `claude-rc.xml` for `windows`;
  `--workspace` is needed only for `windows`).

`--check` writes nothing and exits `1` when a file is stale. Other failures
also exit `1`. See [Install a node](install.md) for how the files are used.

## render roles

```
playpilot render roles --project <name> [--config <fleet repo>] [--repo <project checkout>] [--check]
```

Writes `.claude/agents/playpilot-<agent>.md` for each plugin agent the project's
roles name (agent `playpilot-<agent>`), from definitions built into the CLI,
because cloud sessions do not load plugins. `sync` covers these files.

## sync

```
playpilot sync [--check] --project <name> [--config <fleet repo>] [--repo <project checkout>]
```

Writes, or with `--check` compares, every project-side file `render` produces
(today `.claude/settings.json`). `--check` exits 0 when all match and 1 with a
list of stale files; usage errors exit 64.

## node install

```
playpilot node install --device <name> [--config <fleet repo>] [--workspace <dir>] [--dry-run] [--force]
```

Puts this node's `render node` output in its OS location: the systemd user unit
under `$XDG_CONFIG_HOME/systemd/user/` (or `~/.config/systemd/user/`) on Linux,
the Task Scheduler XML under `%APPDATA%\fleet\` on Windows. It prints the
enable commands and never runs them. An identical file is a no-op; a different
file is refused without `--force`.

## plan

Read-only view of who does what: device, lane, project and role, and what is
uncovered.

```
playpilot plan [--config <dir>]
```

Exit `0`, or `1` when `fleet/` does not load.

## whoami

On a node, resolves a device to its roles, lanes and projects.

```
playpilot whoami --device <name> [--config <dir>]
```

The device defaults to `PLAYPILOT_DEVICE`. Exit `0`, or `1` when the
configuration does not load or the device is unknown or unset.

## claim

Claims a lane item (an issue or pull request) so no other node works on it.

```
playpilot claim <owner/repo#n> [--as <id>] [--stale-hours <n>]
```

The earliest claim wins. A claim with no activity for `--stale-hours`
(default 6) is taken over. Exit `0` when claimed or already yours, `1` when
another node holds it or claimed first.

## release

Releases an item this node holds, with an optional result note.

```
playpilot release <owner/repo#n> [--as <id>] [--note <text>]
```

Exit `0` when released, `1` on failure.

## mail

The fleet mailbox: messages between sessions. Messages are information, never
approval.

```
playpilot mail send  --to <actor> --subject <text> [--re <ref>] (--body <text> | --body-file <path|->)
playpilot mail inbox [--all] [--days <n>]
playpilot mail read  <message-id>
playpilot mail ack   <message-id>
```

Common options: `--repo <owner/name>` (or `PLAYPILOT_MAILBOX_REPO`) and
`--as <actor>` (or `PLAYPILOT_ACTOR`). `--to all` copies a message into every
other mailbox. `inbox` lists unacked messages (`--all` includes acked ones);
`read` and `ack` mark a message read or handled. The Worker at
`PLAYPILOT_MAIL_URL` is used when set, otherwise the GitHub repository. Exit
`0` on success, `1` on failure, `64` on `--help` or a missing verb.

## mcp

A Model Context Protocol server on stdio that exposes the same use cases as
tools: `mail_inbox`, `mail_send`, `mail_read`, `mail_ack`, `task_claim`,
`task_release` and `whoami`.

```
playpilot mcp
```

The actor and mailbox come from the environment (`PLAYPILOT_ACTOR`,
`PLAYPILOT_MAIL_URL` or `PLAYPILOT_MAILBOX_REPO`), never from tool arguments,
so a tool call cannot speak for another actor. Exits `0` when stdin closes.

## playpilot-worker

The optional Cloudflare Worker (`crates/playpilot-worker`) is not a CLI
subcommand. It serves the HTTP API, leases, the mailbox and a remote `/mcp`
endpoint with the same tool names. Build and deploy notes, secrets and
access checks are in
[Central management](../config-and-extensions.md#5-central-management-git-first-worker-optional).
