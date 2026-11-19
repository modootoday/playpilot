# Claiming work

GitHub is the only shared state between devices and cloud sessions.

1. Find candidates: open issues or pull requests with your lane label and no
   `claimed:*` label.
2. Claim: post a comment whose first line is `claim: <device-id> <UTC time>`,
   then add the label `claimed:<device-id>`.
3. Re-read the comments. If an earlier `claim:` comment from another device
   exists, the earliest one wins: remove your label and move on.
4. Finish: comment the result, remove `claimed:<device-id>` and the lane label
   (or keep the lane label and add `needs-owner` if a decision is needed).
5. A claim older than 6 hours with no further comment is stale; any device may
   comment `release: stale claim` and reclaim.
