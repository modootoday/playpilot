# Claiming work

GitHub is the only shared state between devices and cloud sessions. Use
`playpilot claim` / `playpilot release` (or the MCP tools `task_claim` /
`task_release`), which follow these rules.

1. Find candidates: open issues or pull requests with your lane label and no
   `claimed` label.
2. Claim: post a comment whose first line is `claim: <id> <UTC time>`, then add
   the label `claimed`. The id is opaque (`PLAYPILOT_CLAIM_ID`, for example
   `node-a`): public repositories never name a device.
3. Re-read the comments. The earliest open claim wins; if it is not yours,
   comment `release: <id>` and move on.
4. Finish: comment `release: <id>` with the command, commit SHA and outcome,
   remove `claimed` and the lane label (or keep the lane label and add
   `needs-owner` if a decision is needed).
5. A claim with no comment for 6 hours (`Fleet.spec.claims.staleAfter`) is
   stale; any node may comment `release: stale claim` and reclaim.
