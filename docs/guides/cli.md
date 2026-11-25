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
| `PLAYPILOT_POLICY_PLUGINS` | `guard` | policy plugins as `sha256=path` items separated by `;`; checked after the core policy allows a command; unset or blank means none. Needs a build with `--features plugins`; any problem refuses ([plugins](../config-and-extensions.md#4-plugins)) |
| `PLAYPILOT_DEVICE` | `whoami` | device name when `--device` is not given |
| `PLAYPILOT_ACTOR` | `mail`, `mcp`, `claim`, `release` | this session's actor (mailbox name); the claim id when `PLAYPILOT_CLAIM_ID` is unset |
| `PLAYPILOT_CLAIM_ID` | `claim`, `release`, `mcp` | the id written on claims; use one that names no device |
| `PLAYPILOT_MAIL_URL` | `mail`, `mcp`, `claim`, `release`, `events` | the Worker's `/mcp` URL; when set, mail goes through the Worker; the lease endpoint for `--backend worker`; the event source for `events tail` and `events deliveries` |
| `PLAYPILOT_MCP_TOKEN` | `mail`, `mcp`, `claim`, `release`, `events` | bearer token for the Worker's `/mcp` |
| `PLAYPILOT_CLAIM_BACKEND` | `claim`, `release` | `github` (default) or `worker`, when `--backend` is not given |
| `PLAYPILOT_MAILBOX_REPO` | `mail`, `mcp` | `owner/name` of the mailbox repository (GitHub transport) |

`claim`, `release` and the GitHub mail transport call GitHub through the `gh`
CLI and its login. The Worker mail transport, Worker leases, `events tail` and `events deliveries` use `curl`.

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

When `PLAYPILOT_POLICY_PLUGINS` is set, a command the core policy allowed
also goes to each listed `policy-check` plugin in order. A plugin's
`refuse` blocks it; so does a SHA-256 mismatch, a missing file, a trap, an
exhausted fuel or memory limit, a malformed list, or a binary built without
the `plugins` feature.

```
cargo build --release -p playpilot --features plugins
PLAYPILOT_POLICY_PLUGINS="<sha256>=<cache>/<sha256>.wasm" playpilot guard < event.json
```

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
private host values, and outbound `Webhook` resources name a public https
URL, a `PLAYPILOT_WEBHOOK_SECRET_<NAME>` signing secret and dotted type
prefixes; [Outbound webhooks](../config-and-extensions.md#outbound-webhooks)).

```
playpilot validate <fleet-dir>
```

Exit `0` valid, `1` lists problems.

## migrate

Rewrites every resource in a fleet configuration directory from its
`apiVersion` to a target version, one registered step at a time.

```
playpilot migrate [--check] [--force] [--to <apiVersion>] <fleet-dir>
```

- `--to` defaults to the newest version this build knows. Today that is
  `playpilot.io/v1` and no steps exist, so `migrate` only reads and validates
  (the same checks as `validate`) and changes nothing.
- `--check` writes nothing; it lists each file that would change and exits `1`,
  or exits `0` when nothing would.
- Without `--check`, changed files are written in place. A rewrite that only
  bumps `apiVersion` edits that one line and keeps comments. A rewrite that
  changes other fields cannot keep TOML comments; `migrate` names those files
  and writes nothing unless `--force` is given.
- An unknown `apiVersion`, a version newer than the target (no downgrades), a
  missing step or a result that does not validate exits `1` and writes
  nothing. Usage errors exit `64`.

```
$ playpilot migrate --check fleet
migrate: 13 resource(s) already at playpilot.io/v1; nothing to change
```

## render

Writes files from the fleet configuration repository (`--config`, default the
current directory, holding `fleet/` and `playpilot.lock`).

```
playpilot render settings --project <name> [--config <dir>] [--repo <dir>] [--check]
playpilot render prompt   --session <name> [--config <dir>]
playpilot render node     --device <name> [--workspace <dir>] [--config <dir>] [--out <dir> [--check]]
playpilot render plugins  [--config <dir>] [--out <file> [--check]]
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

## render plugins

```
playpilot render plugins [--config <fleet repo>] [--out <file> [--check]]
```

Builds the plugin index (`index.json`, schema
`schemas/plugin-index.v1.schema.json`) from the fleet's `Plugin` resources and
prints it, or with `--out` writes it. Each entry has the plugin's `id`
(`metadata.name`), `kind` (`wasm-component` or `process`), `interface`
(`policy-check`, `event-sink`, `lane-source` or `runtime`), the full `wit`
reference, its `source` pin (https `url`, `sha256`) and its `grants`
(network hosts, secret names). Entries are sorted by id and grants by value,
so the same configuration always gives the same bytes.

The whole index is refused, listing every problem with its file, when a plugin
has no https URL, a digest that is not 64 lower-case hex digits, an interface
outside `playpilot:fleet/<interface>@1.x.y`, an unknown kind or grant, or a
secret given as anything but a name or `{ secretRef = "NAME" }`. `--check`
needs `--out` and exits `1` when the file is stale.

## status

Writes the snapshot the [status page](../status.md) reads: the Worker's public
`/v1/state`, validated and rebuilt from the fields of the `PublicState` subset
in `schemas/worker.v1.openapi.yaml`.

```
playpilot status --from <url|file> [--out docs/status.json]
```

`--from` is an `http(s)://` URL, fetched with `curl` and no credential, or a
local file. A field outside the public subset, a node id that is not an opaque
id, or a malformed count or date is refused, never dropped, so nothing private
reaches the page. Without `--out` the snapshot goes to stdout. Exit `0` when
written, `1` when the state cannot be read or is refused, `64` on usage errors.

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
the Task Scheduler XML under `%USERPROFILE%\fleet\.node\` on Windows. It prints the
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
playpilot claim <owner/repo#n> [--as <id>] [--stale-hours <n>] [--backend github|worker] [--lane <name>]
```

With the `github` backend (the default) the earliest claim comment wins, and
a claim with no activity for `--stale-hours` (default 6) is taken over.

With the `worker` backend the Worker's lease decides. `claim` first takes a
Durable Object lease through the Worker's `task_claim` tool (lane `--lane`,
default `default`; TTL `--stale-hours`, 1 to 24 hours), then mirrors it on the
item: a comment `claim: <id> <time>` with a second line
`lease: <lease id> until <time>`, and the `claimed` label.

- An open lease held by someone else refuses the claim, and the item is not touched.
- Claiming again as the holder renews the lease and posts the new `lease:` line.
- A mirrored claim whose lease is gone (the Worker granted a new one) is ended
  with `release: expired lease`. A claim made without the Worker keeps the stale rule.
- When a claim made without the Worker is still fresh, or another claim lands
  first, the lease is given back.
- The claim id comes from the Worker credential. If `--as` names another id,
  the claim is refused and the lease is given back.
- If GitHub fails after the lease is taken, the lease stays and runs out by its TTL.

Exit `0` when claimed or already yours, `1` when another node holds it,
claimed first or the Worker refused.

## release

Releases an item this node holds, with an optional result note.

```
playpilot release <owner/repo#n> [--as <id>] [--note <text>] [--backend github|worker]
playpilot release --expired <owner/repo#n | owner/repo>
```

With `--backend worker` the lease is released first. A lease that has already
expired is not an error. Then the claim comment and label are released.

`--expired` ends claims whose mirrored lease has run out (the latest
`lease: … until <time>` line is in the past). It comments `release: expired
lease` and removes `claimed`. It runs on one item, or on every open `claimed`
item of a repository. Any node may run it. Claims without a `lease:` line are
left alone.

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
other mailbox. `inbox` lists unacked messages (`--all` includes acked ones).
`read` prints one message from the actor's mailbox (or the broadcast box) the
way `inbox` does, header line then the indented body, and marks it read. It
fails, marking nothing, when the id is not in that mailbox. `ack` marks a
message handled. The Worker at
`PLAYPILOT_MAIL_URL` is used when set, otherwise the GitHub repository. Exit
`0` on success, `1` on failure, `64` on `--help` or a missing verb.

## events tail

Prints the inbound events the Worker stored (GitHub deliveries as
CloudEvents, [Inbound GitHub webhooks](../config-and-extensions.md#inbound-github-webhooks)),
oldest first, through the `/mcp` tool `events_read`.

```
playpilot events tail [--type <prefix>] [--after <seq>] [--limit <n>] [--follow [--interval <secs>]] [--json]
```

Each event is one line: `seq`, time, type, source (without `https://`) and a
summary of the allow-listed data, `repo#number action`, `ref@sha` (7
characters) and `by sender`:

```
12  2026-11-25T10:00:01Z  com.github.pull_request.opened  github.com/octo-org/hello  octo-org/hello#7 opened by octocat
```

`--json` prints each stored CloudEvent as one JSON line instead. `--type`
filters by a type prefix on dot boundaries (`com.github.pull_request` does not
match `com.github.pull_request_review`). `--after` starts after a `seq`
(default `0`, the oldest event kept); `--limit` is 1 to 200 (default 100).

Without `--follow` it reads one page and exits. With `--follow` it keeps
reading from the last `seq`: a full page is followed at once, otherwise it
waits `--interval` seconds (default 15). A network or Worker error while
following is printed to stderr and retried, waiting the interval doubled per
failure up to five minutes; it never ends the command. Stop it with Ctrl-C.

The Worker is `PLAYPILOT_MAIL_URL`, with the token from `PLAYPILOT_MCP_TOKEN`
(or the environment's credential for the Worker host); any valid token may
read events. Event data is printed, never interpreted. Exit `0` on success,
`1` on failure (a read error without `--follow`, or no `PLAYPILOT_MAIL_URL`),
`64` on a usage error.

## events deliveries

Prints the outbound webhook deliveries the Worker keeps
([Outbound webhooks](../config-and-extensions.md#outbound-webhooks)), oldest
first, through the `/mcp` tool `deliveries_read`.

```
playpilot events deliveries [--webhook <name>] [--after <seq>] [--limit <n>] [--json]
```

Each delivery is one line: `seq`, last update, webhook, event type, state
(`pending`, `delivered` or `failed`), attempts, then the last HTTP status, the
next retry and the reason when there are any:

```
4  2026-11-25T10:00:30Z  owner-notify  com.github.push  pending  attempts 2  http 503  next 2026-11-25T10:02:30Z  (HTTP 503)
5  2026-11-25T10:00:31Z  owner-notify  com.github.ping  delivered  attempts 1  http 204
```

`--webhook` keeps one `Webhook` resource; `--after` starts after a `seq`
(default `0`); `--limit` is 1 to 200 (default 100); `--json` prints each row as
the Worker returned it. The cursor for the next page is printed on stderr as
`next: <seq>`. Rows never hold URLs, secrets or bodies. Same Worker and token
as `events tail`; exit `0` on success, `1` on failure, `64` on a usage error.

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
subcommand. It serves the HTTP API, leases, the mailbox, a remote `/mcp`
endpoint with the same tool names plus `events_read` (the stored events, read by
`events tail`) and `deliveries_read` (outbound webhook deliveries, read by
`events deliveries`), signed outbound webhook deliveries from the
`EventsObject` alarm, and inbound GitHub webhooks on
`POST /hooks/github`. Routes and how each authenticates:

| Route | Auth | Host |
| --- | --- | --- |
| `/v1/*` (except public `GET /v1/state`) | Access service token, JWT verified by the Worker | behind Access |
| `POST /mcp` | `Authorization: Bearer`, hash in `PLAYPILOT_MCP_TOKENS` | without Access |
| `POST /hooks/github` | `X-Hub-Signature-256` under `PLAYPILOT_GITHUB_WEBHOOK_SECRET` only | without Access |

Build and deploy notes, secrets and access checks are in
[Central management](../config-and-extensions.md#5-central-management-git-first-worker-optional);
webhook setup is in
[Inbound GitHub webhooks](../config-and-extensions.md#inbound-github-webhooks)
and [Outbound webhooks](../config-and-extensions.md#outbound-webhooks).
