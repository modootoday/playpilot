---
---
# Configuration, interfaces and extensions

## 1. One resource model

Everything the fleet knows is a versioned resource:

```
apiVersion = "playpilot.io/v1"
kind       = Fleet | Environment | Project | Lane | Device | Role | PermissionProfile | Session | Webhook | Plugin
metadata   = { name, labels, annotations }
spec       = kind-specific, plus spec.extensions."x-<plugin>" for plugin-owned fields
```

- Files: one resource per file (TOML, YAML or JSON; all map to the same
  schema). Canon location: the instance's private configuration repository, `fleet/`. Public repositories receive only rendered projections (`playpilot render`), never the source.
- Schema: JSON Schema 2020-12 at
  [`schemas/playpilot.io-v1.schema.json`](../schemas/playpilot.io-v1.schema.json),
  one `if/then` spec schema per kind; published on each release. Editors
  validate with it. It is hand-written for now; later it is generated from the
  Rust types (`schemars`). Tests keep it in step with the kinds `validate` knows
  and check that every file under `examples/` parses.
- Compatibility: `v1` takes additive changes only. Unknown core fields are
  errors; unknown fields under `extensions` are kept and passed to the owning
  plugin. A breaking change ships as `v2` with `playpilot migrate` and both
  versions accepted for one minor release.
- Validation: `playpilot validate` = schema + semantics (references resolve,
  every lane has a capable device, profiles only narrow, no secret values, no
  private host values).

## 2. Core interfaces (ports)

The CLI, the Worker and plugins are adapters around the same ports
(`domain` / `application` / `infrastructure` layering):

| Port | Built-in adapters | Plugin interface |
| --- | --- | --- |
| `ConfigSource` | git (pinned ref), Worker `GET /v1/config` | — |
| `WorkQueue` (list, claim, release, report) | GitHub labels + claim comments, Worker leases | `lane-source` |
| `Runtime` (start, status, stop) | wsl, windows, claude-cloud, cf-container | `runtime` |
| `PolicyCheck` | core guard (always first) | `policy-check` (refuse-only) |
| `EventSink` | outbound webhooks, GitHub comments | `event-sink` |
| `VcsHost` | GitHub | later: GitLab, Forgejo |
| `SecretRef` | env var, OS keychain, Worker binding | — (names only, never values) |

## 3. Events and webhooks

- Format: CloudEvents 1.0 (`type` = `fleet.<noun>.<verb>`, e.g.
  `fleet.run.finished`, `fleet.claim.created`, `fleet.claim.stale`,
  `fleet.node.offline`, `fleet.config.applied`).
- Outbound: `Webhook` resources, signed with the Standard Webhooks scheme
  (`webhook-id`, `webhook-timestamp`, `webhook-signature`), retries with
  backoff, idempotent by event id.
- Inbound: GitHub webhooks (and other sources) arrive only at the Worker,
  are verified, normalised to the same CloudEvents, then fanned out.
- Events carry data, never commands. A node maps an event to a fixed local
  action; no shell text from an event is ever executed.

## 4. Plugins

| Kind | Use | Isolation |
| --- | --- | --- |
| `wasm-component` (default) | lane sources, policy checks, sinks, runtimes | WASI sandbox; network and secrets only as granted in the `Plugin` resource |
| `process` | things that need the OS (e.g. a GUI runner) | JSON-RPC 2.0 over stdio, same method names as the WIT interfaces; runs as the node user |

Interfaces: `schemas/playpilot-plugin.wit` (`playpilot:fleet@1.0.0`).

Invariants:
1. Plugins are pinned by URL + SHA-256; adding or changing one is a
   `needs-human` change.
2. Plugins can add work, observe events or refuse actions. They can never
   widen permissions, skip the core guard, or read secret values they were not granted.
3. A failing plugin fails closed for `policy-check`, open (logged) for `event-sink`.

Claude Code plugins (skills, agents, hooks for the CLI) stay a separate,
local-only convenience; cloud sessions do not load them, so nothing in the
fleet depends on them.

## 5. Central management: Git first, Worker optional

| | Phase A: Git only | Phase B: Git + Worker |
| --- | --- | --- |
| Authoring | PRs to `fleet/` | same (the Worker never authors config) |
| Distribution | nodes pull a pinned ref | Worker serves compiled config with ETag; nodes fall back to Git |
| Claims | label + earliest comment wins | Durable Object lease (atomic, TTL), mirrored to the label |
| Live state | GitHub only | heartbeats, node/lane dashboard |
| Runtime switches | `ops` in the file | Worker `ops` (pause node/lane, lower capacity) — narrow-only; effective value is the narrower of file and Worker |
| Cloud sessions | read Git (no network change) | still read Git |

Worker HTTP API (OpenAPI 3.1, [`schemas/worker.v1.openapi.yaml`](../schemas/worker.v1.openapi.yaml);
every call but the public `GET /v1/state` subset sends the
`CF-Access-Client-Id` / `CF-Access-Client-Secret` headers):
`GET /v1/config`, `POST /v1/leases`, `DELETE /v1/leases/{id}`,
`POST /v1/nodes/{id}/heartbeat`, `POST /v1/events`, `GET /v1/state`.
Access: Cloudflare Access service tokens per node. The Worker holds a GitHub
App installation (issues/labels write, contents read) and, only for waking
sessions, per-task routine tokens (below); no other Claude credentials.
The Worker lives in `crates/playpilot-worker` (Durable Objects `LeaseObject`,
`NodeObject`, `BusObject`, plus a remote MCP endpoint `POST /mcp` with the
local `playpilot mcp` tool names). Build with `worker-build --release`.

Access check. The Worker does not trust Access headers as such: on `/v1/*`
it verifies `Cf-Access-Jwt-Assertion` (RS256, signed by a key from
`https://<team domain>/cdn-cgi/access/certs`, cached per isolate for 10
minutes; `iss` is `https://<team domain>`, `aud` contains the application's
AUD tag, `exp`/`nbf` with 60 s skew) and takes the caller from the token's
`common_name`, the service token client id, which keys
`PLAYPILOT_IDENTITIES`. Set `PLAYPILOT_ACCESS_TEAM_DOMAIN` (bare host, e.g.
`example.cloudflareaccess.com`) and `PLAYPILOT_ACCESS_AUD`; while either is
unset, or the token fails any check, every `/v1/*` call except the public
`GET /v1/state` gets `401`.

Compiled config. Text bindings stop near 5.1 kB, so the compiled config goes
in a KV namespace bound as `CONFIG`, key `config.json`. When that binding
exists it is authoritative (a missing key is an error, not a fall back);
without it the Worker reads the `PLAYPILOT_CONFIG` var or secret.

### Fleet mailbox

Transports. `playpilot mail` and the local `playpilot mcp` use the Worker when
`PLAYPILOT_MAIL_URL` is set (e.g. `https://mail.example.invalid/mcp`), and
otherwise the GitHub transport (draft pull requests in
`PLAYPILOT_MAILBOX_REPO`). The Worker transport speaks JSON-RPC to `/mcp`
through `curl`; the actor is `PLAYPILOT_ACTOR`, sent as the `as` argument.

Auth. Access can only read a service token from its own header pair, while
cloud sessions can only inject `Authorization: Bearer <secret>`. So `/mcp` is
served on a separate host without Access, and the Worker checks the bearer
token itself (on any host; Access headers are ignored on `/mcp`). The secret
`PLAYPILOT_MCP_TOKENS` maps the SHA-256 hex of each token to what it allows:

```json
{"<sha256 hex of token>": {"actors": ["node-a", "node-b"], "claim": "cloud-a"}}
```

A missing or unknown token gets a bare `401`. One token may stand for
several actors (cloud sessions share one environment); a call then must name
its actor with `as`, which must be in `actors`. With one actor, `as` is
implied. `whoami` lists the allowed actors. The lease holder is `claim`, or
the actor when unset. Locally, `PLAYPILOT_MCP_TOKEN` supplies the token; in a
cloud session the environment's credential for the Worker host does.
Route only `/mcp*` to the Worker on that host: `/v1/*` still relies on
Access in front of it.

Wake. A cloud session can only be woken by firing its scheduled task, which
starts a new session. The optional secret `PLAYPILOT_WAKE` maps actors to a
routine and its per-task token:

```json
{"node-a": {"routine": "<routine id>", "token": "<per-task token>"}}
```

After `mail_send` stores its copies, each recipient with an entry is woken
with the text `playpilot mail: <n> unacked message(s) for <actor>; read them
with the playpilot mail tools (mail_inbox) and handle them.` Message content
never goes into the fire text; the new session reads it with `mail_inbox`.
`BusObject` fires at most once per actor per 10 minutes while that actor
still has unacked mail; acking everything resets the window. Upstream limits
are 30 fires per hour per task and 100 per hour per account, with no
idempotency key; a failed or `429` fire is logged and never fails the send.

## 6. Change control

| Change | Path |
| --- | --- |
| Narrowing (pause, fewer lanes, stricter profile) | normal PR, auto-merge |
| Adding a device, role, session, webhook or plugin; widening any profile | `needs-human`, measured, then auto-merge per C7 |
| Worker `ops` switch | narrow-only at runtime, logged as `fleet.config.applied` |

## 7. CLI surface

Implemented: `guard`, `history-check`, `surface-check`, `validate`,
`render settings | prompt | node`, `sync`, `node install`, `plan`, `whoami`,
`claim`, `release`, `mail send | inbox | read | ack` and `mcp`. Usage, exit codes and environment
variables are in the [CLI reference](guides/cli.md).

Planned: `migrate`, `events tail`, and `render` for role files.

- `plan`: who does what (device × lane × project × role), and what is uncovered.
- `render`: writes `.claude/settings.json` (`render settings`), systemd units
  and Task Scheduler XML (`render node`) and scheduled-task prompts
  (`render prompt`) from the config; role files are planned.
- `whoami`: on a node, resolves its device id to roles, lanes and projects.
- `sync --check` (implemented): lists every project-side file `render` would
  produce that differs from the checkout (exit 1); `sync` writes them.
- `node install` (implemented): puts `render node` output into the OS location
  (systemd user unit, Task Scheduler XML) and prints, never runs, the enable
  commands; `--dry-run` previews, `--force` replaces a file that differs.
- `migrate` (planned): rewrites resources from one API version to the next.
- `events tail` (planned): follows fleet CloudEvents.

Releases come from the owner-run `release` workflow
(`.github/workflows/release.yml`): Linux x86_64 and aarch64 and Windows x86_64
archives with `SHA256SUMS` and build provenance attestations. crates.io and
container images are follow-ups.
