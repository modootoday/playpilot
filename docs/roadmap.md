---
---

# Roadmap to 1.0

playpilot is complete (1.0) when every row below without the `optional` tag
is done under governance G10. Rows run top to bottom unless a row names
other dependencies. Owner rows (level A2) are prepared by agents and finished
by the owner; agents skip them and continue.

Status: `done`, `next` (unblocked), `blocked` (waiting on a dependency or the
owner), `optional`.

## Done (phase A and the stage Worker)

| Row | What | Pull requests |
| --- | --- | --- |
| D1 | guard, history-check, surface-check, validate | #2, #12–#17 |
| D2 | Plugin marketplace, docs site, brand | #3–#9 |
| D3 | Fleet mailbox over GitHub, then the Worker; MCP server | #15, #21, #22, #29 |
| D4 | render settings, roles, prompt, node; sync; node install | #19, #26, #31, #34–#37 |
| D5 | claim / release; plan / whoami | #23, #24 |
| D6 | v1 resource schema and Worker OpenAPI | #25 |
| D7 | Phase B Worker: leases, nodes, bus, Access JWT, KV config (stage) | #28, #30 |
| D8 | Release workflow and CLI reference | #32 |

## To 1.0

| Row | Level | Depends on | Work | Accepted when |
| --- | --- | --- | --- | --- |
| R1 | A0 | — | **Worker-backed claims** (done, #39; live on stage 2026-11-24: claim showed lease + label, expiry released the label): `playpilot claim --backend worker` takes a Durable Object lease, then mirrors it as the `claimed` label and claim comment; `release` frees both; a lease that expires is released on the item | Host tests for the mirror; a live claim on stage shows lease + label; expiry releases the label |
| R2 | A2 | — | **Device service tokens**: the owner creates one Access service token per node and sets `PLAYPILOT_IDENTITIES` (client id → claim id) | `/v1/leases` with a node token returns 201 on stage |
| R3 | A0 | R2 | **Node heartbeat**: `playpilot node heartbeat` (and the systemd / Task Scheduler units call it every 5 minutes); a heartbeat that renews leases also posts the new `lease: … until` line on each item, so `release --expired` keeps reading the right expiry | `/v1/state` shows the node online; offline after the window; a renewed lease is not swept |
| R4 | A1 | — | **Inbound webhooks** (done, #49) on the hooks host: verify the GitHub signature (`X-Hub-Signature-256`), turn the delivery into a CloudEvent, store it, never execute payload text | Signature tests; a replayed or unsigned delivery is refused; a real GitHub ping is stored on stage (owner steps: [Inbound GitHub webhooks](config-and-extensions.md#inbound-github-webhooks)); measured on stage 2026-11-25: GitHub ping 202, redelivery 200 duplicate |
| R5 | A0 | R4 | **Outbound events**: `Webhook` resources get Standard Webhooks signed deliveries with retries and de-duplication | Tests with a fixed key; a local receiver sees signed, retried, de-duplicated events |
| R6 | A0 | R4 | **`events tail`**: follows the stored events through the Worker | Lists events in order; filter by type |
| R7 | A0 | — | **`migrate`** (done, #40): framework that rewrites resources between API versions; v1 → v1 is a no-op that only validates | Tests; `migrate --check` on the fleet is clean |
| R8 | A0 | — | **Public status page** (done, #43): `/v1/state` public subset rendered on the docs site (no device names) | Page shows lanes, open claims count, last run per project. [status page](status.md), `playpilot status` |
| R9 | A0 | R11 | **Install redirects** (`get.` host): `install.sh`, `install.ps1`, `/v/<version>/<target>` → release asset + SHA-256 | Installs a released binary on Linux and Windows |
| R10 | A0 | — | **Plugin index** (`plugins.` host): generated from `Plugin` resources, pinned by URL and SHA-256 | Index validates against the schema. **done** (#41): `render plugins` |
| R11 | A2 | — | **First release v0.1.0**: owner creates the `release` environment, applies `v-tags` (v tags cannot be moved or deleted; creating one is gated by the `release` environment reviewer, because a personal-account ruleset cannot list GitHub Actions as a bypass actor), runs the workflow | Release with three archives, SHA256SUMS, attestations |
| R12 | A0 | R11 | **Pin by version**: fleet `playpilot.lock` holds a version; `--version` prints version and rev; the setup script and hooks install the release asset instead of building | Fresh container installs in under 30 s |
| R13 | A1 | — | **Plugin host**: WASM component host (WIT `playpilot:fleet@1.0.0`) for `policy-check` (refuse only) and `event-sink`; fail-closed for policy, log-and-continue for sinks | Example plugins of both kinds pass; a crashing policy plugin blocks. **done** (#44): `crates/playpilot-plugins`, `guard` with `PLAYPILOT_POLICY_PLUGINS` |
| R14 | A1 | R12 | **Windows node**: Task Scheduler task from `node install` runs the Remote Control server; the guard hook runs on Windows | `needs-windows-run` item with the measured run; measured 2026-11-25 on the Windows node: task Running, Remote Control connected, guard refuses push main / tag (exit 2) and allows topic push (exit 0) — kinekino#60; remaining: re-run after R12 with the released binary |
| R15 | A2 | R2–R8 | **Production**: owner deploys the same build to the production hosts and Access | Probes on production match stage |

## Optional

| Row | Level | Work |
| --- | --- | --- |
| O1 | A2 | Paid mode: Cloudflare container runtime with AI Gateway budgets |
| O2 | A2 | Instant wake: owner-created scheduled tasks with API triggers, wired into `PLAYPILOT_WAKE` |
| O3 | A0 | crates.io and container image publishing in the release workflow (publishing itself stays A2) |
