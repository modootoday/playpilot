# Operations domain: playpilot.io

The product keeps its name (`playpilot`, schema group `playpilot.io`).
`playpilot.io` is the owner's deployment of it: the public face and the control
endpoints for the modootoday repositories. Swapping the domain changes one
resource (`Fleet.spec.domain`), nothing else.

The zone is already on Cloudflare nameservers, so Workers custom domains,
Access, R2 and Email Routing work without moving DNS.

## Host map

| Host | Serves | Access | Backed by |
| --- | --- | --- | --- |
| `playpilot.io` | Public fleet status: projects, lanes, open claims, last run per lane, links to PRs. Static, rebuilt from GitHub data | Public, read-only | Worker static assets |
| `api.playpilot.io` | `/v1/config`, `/v1/leases`, `/v1/nodes/{id}/heartbeat`, `/v1/events`, `/v1/state` | Access service token per node; `GET /v1/state` public subset | Worker + Durable Objects |
| `hooks.playpilot.io` | Inbound webhooks (GitHub App, plugin sources). Verify signature → CloudEvent → fan out | Signature required; WAF rate limit | Worker + Queue |
| `ops.playpilot.io` | Owner console: pause/drain node or lane, lower capacity, view stale claims | Access, owner identity only | Worker |
| `get.playpilot.io` | `/install.sh`, `/install.ps1`, `/v/<version>/<target>` → redirect to the GitHub Release asset + SHA-256 | Public | Worker (redirects only, no binaries hosted) |
| `plugins.playpilot.io` | `index.json`: plugin id → URL, SHA-256, interface, grants | Public, read-only; changes by PR | Worker static assets (generated from Git) |
| `security@playpilot.io` | Security contact in every `SECURITY.md` | Mail forwarded to the owner | Email Routing |

Not public hosts: the sccache bucket (R2, private, S3 API only) and any
container dispatcher (internal binding).

## Rules

- Everything served publicly is generated from public GitHub data; nothing
  private (hostnames, device specs, logs) is published.
- Nodes never accept inbound connections. They call `api.` outbound; Remote
  Control stays outbound to Anthropic.
- Webhook payloads are data. The Worker never runs commands from them.
- Owner-only: DNS records, Access policies, Worker secrets, GitHub App keys.
  They are declared in `infra/` files and applied from the owner's machine
  (same model as rulesets, common rules C2.7).

## GitHub side

- Verify `playpilot.io` for the modootoday organisation (verified badge;
  lets notifications be limited to the verified domain).
- GitHub App for the fleet: homepage `https://playpilot.io`, webhook URL
  `https://hooks.playpilot.io/github`, permissions: issues and pull requests
  write (labels, comments), contents read, checks read. No contents write,
  no administration, no secrets. Commits stay authored by the owner noreply
  address; the App only labels, comments and leases.
- Every repository: `SECURITY.md` → `security@playpilot.io`, homepage →
  `https://playpilot.io/<project>`.

## Zone hardening

DNSSEC on; HSTS on; minimum TLS 1.2; Always Use HTTPS; Access on `ops.` and
`api.` (except the public state subset); a rate-limiting rule on `hooks.`;
no wildcard records.

## Stage environment

A second zone (`<stage-domain>`, named only in the private instance config)
mirrors every public host one level deep, because Cloudflare's universal
certificate covers `*.<zone>` but not `*.*.<zone>`:

| Public | Stage |
| --- | --- |
| `playpilot.io` | `playpilot.<stage-domain>` |
| `api.playpilot.io` | `playpilot-api.<stage-domain>` |
| `hooks.playpilot.io` | `playpilot-hooks.<stage-domain>` |
| `ops.playpilot.io` | `playpilot-ops.<stage-domain>` |
| `get.playpilot.io` | `playpilot-get.<stage-domain>` |
| `plugins.playpilot.io` | `playpilot-plugins.<stage-domain>` |
| (per pull request preview) | `playpilot-pr-<n>.<stage-domain>` |

- The whole stage zone sits behind Access (owner only); nothing there is public.
- Stage has its own Worker environment, Durable Object namespaces, R2 buckets,
  Access service tokens and its own GitHub App, so stage webhooks and leases
  never touch production state.
- The stage domain is an internal host: it never appears in public repositories,
  commits or pull requests. Public files use the placeholder above; the
  deny-terms secret lists the real name.
- Promotion is by version: a release is deployed to stage first, then the same
  build is deployed to `playpilot.io` by the owner.
