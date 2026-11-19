# Configuration, interfaces and extensions

## 1. One resource model

Everything the fleet knows is a versioned resource:

```
apiVersion = "playpilot.io/v1"
kind       = Fleet | Project | Lane | Device | Role | PermissionProfile | Session | Webhook | Plugin
metadata   = { name, labels, annotations }
spec       = kind-specific, plus spec.extensions."x-<plugin>" for plugin-owned fields
```

- Files: one resource per file (TOML, YAML or JSON; all map to the same
  schema). Canon location: the instance's private configuration repository, `fleet/`. Public repositories receive only rendered projections (`playpilot render`), never the source.
- Schema: JSON Schema 2020-12 generated from the Rust types (`schemars`), published
  at `schemas/fleet.v1.schema.json` and on each release. Editors validate with it.
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

Worker HTTP API (OpenAPI 3.1, `schemas/worker.v1.openapi.yaml`):
`GET /v1/config`, `POST /v1/leases`, `DELETE /v1/leases/{id}`,
`POST /v1/nodes/{id}/heartbeat`, `POST /v1/events`, `GET /v1/state`.
Access: Cloudflare Access service tokens per node. The Worker holds a GitHub
App installation (issues/labels write, contents read) and no Claude credentials.

## 6. Change control

| Change | Path |
| --- | --- |
| Narrowing (pause, fewer lanes, stricter profile) | normal PR, auto-merge |
| Adding a device, role, session, webhook or plugin; widening any profile | `needs-human`, measured, then auto-merge per C7 |
| Worker `ops` switch | narrow-only at runtime, logged as `fleet.config.applied` |

## 7. CLI surface

`playpilot validate | plan | render | whoami | claim | release | sync --check | node install | guard | migrate | events tail`

- `plan`: who does what (device × lane × project × role), and what is uncovered.
- `render`: writes `.claude/settings.json`, role files, systemd units,
  Task Scheduler XML and scheduled-task prompts from the config.
- `whoami`: on a node, resolves its device id to roles, lanes and projects.
