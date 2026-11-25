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
  backoff, idempotent by event id
  ([Outbound webhooks](#outbound-webhooks)).
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

Plugin index: `playpilot render plugins --config <fleet repo> --out index.json`
builds `index.json` (schema `schemas/plugin-index.v1.schema.json`) from the
`Plugin` resources: per plugin its id (`metadata.name`), kind, interface, WIT
reference, source URL and SHA-256 pin, and its grants (network hosts and
secret names only), sorted by id with a trailing newline, so the same
configuration always gives the same bytes. A plugin without an https URL or a
64-hex SHA-256, with an unknown interface, kind or grant, or with anything
but a secret name or `secretRef` in its secrets refuses the whole index. The
plugins host (`plugins.`) serves this file as is; hosts and nodes resolve a
plugin through it and verify the digest before loading it. `--check` in CI
keeps the published index in step with the configuration.

Plugin host (`crates/playpilot-plugins`): runs `wasm-component` plugins
that export `policy-check` or `event-sink` (the other interfaces are not
hosted yet and refuse to load). The host loads an index entry from the local
cache `<cache>/<sha256>.wasm` (fetching is a separate step), checks the
SHA-256 against the pin before compiling, and refuses a mismatch, a missing
file or a file over 16 MiB. The sandbox links nothing: no WASI filesystem,
clock, network or host calls, so a component that imports anything fails to
instantiate. Every call gets a fresh instance, a fixed fuel budget (50 million
units) and a 16 MiB memory cap; running out of either traps. Grants are not
enforced yet, so an entry that grants network hosts or secrets is refused for
both interfaces.

| Interface | Plugin answers | Plugin fails, traps or hits a limit |
| --- | --- | --- |
| `policy-check` | `allow` lets the command through; `refuse(reason)` blocks it with the reason | blocks the command (fail closed) |
| `event-sink` | `ok` | logged on stderr, the caller continues and the other sinks still get the event (fail open) |

`playpilot guard` consults policy plugins only after the core policy allowed
a `Bash` or `PowerShell` command, so a plugin can add a refusal but never
allow what the core refused. The plugins are listed in
`PLAYPILOT_POLICY_PLUGINS` as `sha256=path` items separated by `;`
([CLI reference](guides/cli.md#environment-variables)). The host is behind the
`plugins` cargo feature of `playpilot` (off by default, it adds wasmtime to
the build): `cargo build --release -p playpilot --features plugins`. A build
without the feature refuses every shell command while the variable is set,
rather than ignore it.

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
`POST /v1/nodes/{id}/heartbeat`, `POST /v1/events`, `GET /v1/state`;
and, signed by GitHub instead of Access, `POST /hooks/github`
([below](#inbound-github-webhooks)).
Access: Cloudflare Access service tokens per node. The Worker holds a GitHub
App installation (issues/labels write, contents read) and, only for waking
sessions, per-task routine tokens (below); no other Claude credentials.
The Worker lives in `crates/playpilot-worker` (Durable Objects `LeaseObject`,
`NodeObject`, `BusObject`, `EventsObject`, plus a remote MCP endpoint `POST /mcp` with the
local `playpilot mcp` tool names, `events_read` and `deliveries_read`). Build with `worker-build --release`.

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

### Inbound GitHub webhooks

`POST /hooks/github` takes GitHub webhook deliveries. Its only credential is
the `X-Hub-Signature-256` header: an HMAC-SHA256 of the raw body under the
Worker secret `PLAYPILOT_GITHUB_WEBHOOK_SECRET`, compared in constant time.
Access and bearer tokens play no part, and the path is served on whatever
host routes to the Worker, so route `/hooks/*` on a host without Access
(GitHub cannot present an Access token).

| Case | Response |
| --- | --- |
| Secret not set (fail closed; nothing is accepted unsigned) | `503` |
| Body over 1 MiB | `413` |
| Signature missing or wrong | `401` |
| Content type other than `application/json` | `415` |
| Missing or malformed `X-GitHub-Event` / `X-GitHub-Delivery`, body not a JSON object | `400` |
| Stored | `202` `{"id": "<delivery>", "status": "accepted"}` |
| Delivery id already stored (re-delivery or replay) | `200` `{"id": "<delivery>", "status": "duplicate"}` |

A stored delivery is a CloudEvent: `id` is `X-GitHub-Delivery`, `source` is
`https://github.com/<owner>/<repo>` (the organization or the app
installation when there is no repository), `type` is
`com.github.<event>[.<action>]`, `time` is the receipt time, and `data` is an
allow-listed projection: `repository`, `number`, `action`, `sender`, `ref`,
`sha`, `labels` (names) and `state`. Titles, bodies, comments, commit
messages and every other field are dropped; payload text is never stored,
executed or interpreted. Events go to the `EventsObject` Durable Object
(binding `EVENTS`), which keeps the newest 1000 events and none older than
14 days, and remembers delivery ids for 14 days, so a replay inside that
window is answered as a duplicate. GitHub signatures carry no timestamp,
so a captured delivery replayed after 14 days would be stored again.

Reading events. The `/mcp` tool `events_read` (Worker only; the local
`playpilot mcp` has no event log) returns stored events oldest first:

| Argument | Meaning |
| --- | --- |
| `after` | cursor: only events with a larger `seq` (default `0`, the oldest kept) |
| `limit` | events per call, default 100; more than 200 is capped at 200, `0` is refused |
| `type` | type prefix on dot boundaries: `com.github.pull_request` matches `com.github.pull_request` and `com.github.pull_request.opened`, not `com.github.pull_request_review` |

`structuredContent` is `{"events": [{"seq", "received_at", "type", "event"}],
"next"}`, where `event` is the stored CloudEvent and `next` is the cursor for
the following call. Any valid `/mcp` bearer token may read: events are
fleet-wide and already reduced to the allow-list above, so no extra grant is
needed; an `as`, when given, must still be one of the token's actors. No new
public route serves the log. `playpilot events tail` is the CLI for it.

Owner setup (A2; agents never set the secret or create the hook):

1. Generate a random secret on your own machine and enter it only at the
   prompt: `wrangler secret put PLAYPILOT_GITHUB_WEBHOOK_SECRET --env stage`
   (run from `crates/playpilot-worker`).
2. Deploy so the `v2` migration creates `EventsObject`, and route
   `/hooks/*` on a host without Access.
3. Create a repository or organization webhook (or set the webhook of a
   GitHub App): payload URL `https://<hooks host>/hooks/github`, content type
   `application/json`, the same secret, SSL verification on, and the events
   you want. GitHub sends a `ping` first; it is stored as `com.github.ping`.
4. Check the hook's "Recent deliveries" page: `202` for the ping, and
   "Redeliver" answers `200` with `duplicate`.

### Outbound webhooks

A `Webhook` resource with `direction = "outbound"` (the default) receives
every newly stored inbound event whose type matches its filter, signed with
the [Standard Webhooks](https://www.standardwebhooks.com/) scheme:

```toml
apiVersion = "playpilot.io/v1"
kind = "Webhook"
[metadata]
name = "owner-notify"
[spec]
direction = "outbound"
url = "https://hooks.example.com/playpilot"   # or { secretRef = "PLAYPILOT_WEBHOOK_URL_OWNER" }
signing = { scheme = "standard-webhooks", secretRef = "PLAYPILOT_WEBHOOK_SECRET_OWNER" }
format = "cloudevents/1.0"
events = ["com.github.pull_request", "com.github.check_suite.completed"]
```

| Field | Rule (`playpilot validate`, repeated by the Worker before each send) |
| --- | --- |
| `url` | `https://` only; no user info, no IPv6 literal, no loopback, private, link-local or reserved IPv4, no single-label host, `localhost`, `.local`, `.internal`, `.lan` or `.home.arpa`. Or `{ secretRef = "PLAYPILOT_WEBHOOK_URL_<NAME>" }`, a Worker secret holding such a URL |
| `signing.secretRef` | the name of a Worker secret binding `PLAYPILOT_WEBHOOK_SECRET_<NAME>` holding `whsec_<base64>` (at least 16 key bytes); never the value |
| `events` | at least one type prefix on dot boundaries, the `events_read` syntax: `com.github.pull_request` matches `com.github.pull_request.opened`, not `com.github.pull_request_review` |

The Worker reads the targets from the compiled config (`CONFIG` /
`PLAYPILOT_CONFIG`). A webhook paused by `ops` (file or Worker) gets no new
deliveries, and its queued ones fail at their next attempt; an inbound one,
or one that breaks a rule above, is ignored.

Delivery semantics:

- **Queue**: when `POST /hooks/github` stores a new event, `EventsObject`
  queues one delivery per matching webhook, keyed by (webhook, event id). A
  re-delivered inbound event is a duplicate and queues nothing, and the key
  refuses a second row anyway, so one event never yields two `webhook-id`s.
- **Request**: `POST` to the URL with the stored CloudEvent JSON as the body
  (`content-type: application/json`), `webhook-id: msg_<32 hex>` (derived
  from the webhook and event id, the same on every attempt),
  `webhook-timestamp` (Unix seconds of this attempt) and `webhook-signature:
  v1,<base64 HMAC-SHA256 of "{id}.{timestamp}.{body}">` under the decoded
  secret. Redirects are not followed; each request is aborted after 10 s.
- **Outcome**: a `2xx` answer delivers. `410 Gone` fails the delivery and
  disables that webhook at that URL (later deliveries fail unsent) until the
  URL changes. Anything else, a timeout or a network error is retried after
  30 s, 2 min, 10 min, 1 h and 6 h; after the sixth attempt the delivery
  fails. The alarm sends at most one delivery per webhook per run, oldest
  first, and re-arms at once while more are due.
- **Fail closed**: a missing secret binding, a value that is not
  `whsec_<base64>`, a URL binding that is unset or breaks the rules, or a
  webhook that is no longer configured fails the delivery without sending
  anything. Secret values never appear in state, logs or replies.
- **Crash safety**: an attempt is booked (and its retry scheduled) before
  the request goes out, so a lost outcome is retried with the same
  `webhook-id`; receivers de-duplicate on it, as the scheme expects.
- **Retention**: pending deliveries stay until they finish; finished ones are
  kept for 14 days, at most the newest 1000.

Reading delivery state. The `/mcp` tool `deliveries_read` (Worker only)
takes `after` (a delivery `seq`), `limit` (default 100, capped at 200) and
`webhook` (a resource name); `structuredContent` is `{"deliveries": [{"seq",
"id", "webhook", "event_id", "type", "state", "attempts", "last_status",
"detail", "next_at", "created_at", "updated_at"}], "next"}`, with `state`
one of `pending`, `delivered`, `failed`. Rows never carry URLs, secrets or
bodies, so any valid `/mcp` token may read them. `playpilot events
deliveries` is the CLI for it.

Owner steps to add a target (A2; agents never set secrets, deploy or create
the receiving endpoint):

1. Create the receiving endpoint and generate its secret there, in the
   `whsec_<base64>` form (for example `whsec_` followed by 24 or more random
   bytes in base64).
2. From `crates/playpilot-worker`, enter it only at the prompt:
   `wrangler secret put PLAYPILOT_WEBHOOK_SECRET_<NAME> --env stage`. To keep
   the URL out of the configuration as well, also
   `wrangler secret put PLAYPILOT_WEBHOOK_URL_<NAME> --env stage` and use
   `url = { secretRef = "PLAYPILOT_WEBHOOK_URL_<NAME>" }`.
3. Add the `Webhook` resource to the fleet configuration (a `needs-human`
   change, §6), run `playpilot validate`, and publish the compiled config to
   the Worker's `CONFIG` namespace.
4. Trigger an inbound event (for example "Redeliver" a GitHub ping to a new
   delivery, or any matching repository activity) and check
   `playpilot events deliveries --webhook <name>`: `delivered` with `http
   2xx`, or the retry and its reason.

## 6. Change control

| Change | Path |
| --- | --- |
| Narrowing (pause, fewer lanes, stricter profile) | normal PR, auto-merge |
| Adding a device, role, session, webhook or plugin; widening any profile | `needs-human`, measured, then auto-merge per C7 |
| Worker `ops` switch | narrow-only at runtime, logged as `fleet.config.applied` |

## 7. CLI surface

Implemented: `guard`, `history-check`, `surface-check`, `validate`, `migrate`,
`render settings | roles | prompt | node | plugins`, `sync`, `node install`, `plan`, `whoami`,
`claim`, `release`, `mail send | inbox | read | ack`, `events tail | deliveries` and `mcp`. Usage, exit codes and environment
variables are in the [CLI reference](guides/cli.md).

- `plan`: who does what (device × lane × project × role), and what is uncovered.
- `render`: writes `.claude/settings.json`, role files, systemd units,
  Task Scheduler XML and scheduled-task prompts from the config (implemented:
  `render settings`, `render roles`, `render prompt`, `render node`), and the
  plugin index (`render plugins`, §4).
  `render roles` writes `.claude/agents/playpilot-<name>.md` for each plugin
  agent the project's roles name, from the definitions built into the CLI,
  because cloud sessions do not load plugins.
- `whoami`: on a node, resolves its device id to roles, lanes and projects.
- `sync --check` (implemented): lists every project-side file `render` would
  produce (`.claude/settings.json` and the role files) that differs from the
  checkout (exit 1); `sync` writes them.
- `node install` (implemented): puts `render node` output into the OS location
  (systemd user unit, Task Scheduler XML) and prints, never runs, the enable
  commands; `--dry-run` previews, `--force` replaces a file that differs.
- `migrate` (implemented): rewrites resources from their API version to the
  newest (or `--to`) through a registry of one-function steps; with only
  `playpilot.io/v1` today it validates and changes nothing. `--check` lists
  files that would change (exit 1); a rewrite that would drop TOML comments
  needs `--force`.
- `events tail` (implemented): prints and follows the stored inbound events through the Worker (`events_read` on `/mcp`).

Releases come from the owner-run `release` workflow
(`.github/workflows/release.yml`): Linux x86_64 and aarch64 and Windows x86_64
archives with `SHA256SUMS` and build provenance attestations. crates.io and
container images are follow-ups.
