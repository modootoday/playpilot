---
---
# Fleet status

The public subset of the Worker's live state (`GET /v1/state`): lanes, open
claims, nodes online and the last run per project. It holds counts and
opaque ids only; no item titles, repositories or device names. Node ids are
not listed here.

<div id="fleet-status" data-src="{{ '/status.json' | relative_url }}" aria-live="polite">
<p>Loading status…</p>
<noscript><p>Status needs JavaScript; the snapshot is <a href="{{ '/status.json' | relative_url }}">status.json</a>.</p></noscript>
</div>
<script src="{{ '/assets/playpilot/status.js' | relative_url }}" defer></script>

The page reads a snapshot written by
[`playpilot status`](guides/cli.md#status), which keeps only the public fields
of the [Worker API](https://github.com/modootoday/playpilot/blob/main/schemas/worker.v1.openapi.yaml)
and refuses any other field. Without a snapshot the page shows "status
unavailable". A value the snapshot does not carry is shown as "unknown".
