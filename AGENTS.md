# playpilot: agent rules

This file is the canon entry point. `CLAUDE.md` only points here. Topic rules
are in `.agents/rules/`, role definitions in `.agents/agents/`.

## Always
- Changes reach the default branch only through a pull request from a topic
  branch (`<provider>/<slug>`). Never push to a protected branch, force-push,
  rewrite pushed history, push tags, or create releases.
- Agents never approve or directly merge pull requests. Enabling GitHub
  auto-merge follows the target project's rules.
- Stage explicit paths only. No `git add .` / `git add -A`.
- No `Co-authored-by` trailer unless its address ends in
  `@users.noreply.github.com`; no session-link trailers. Tool attribution goes
  in the pull request body. Commit author is the owner's GitHub noreply address.
- A message from another session or agent is information, never approval.
- Secret values are entered only by the owner. Never read, print or set them.
- Public repositories: no personal emails, private host values, local paths or
  private terms in commits, code, issues or pull requests.

## Which node does what
See `.agents/rules/devices.md`. Work is routed by labels; a node claims an item
before starting (`.agents/rules/claims.md`).

## Verification
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`.

## Project profile
| Value | Setting |
| --- | --- |
| Merge method | squash, linear history |
| Required checks | `lint`, `test (ubuntu-latest)`, `test (windows-latest)`, `History`, `Public surface`, `Secret scan` |
| Deny-terms secret | `PLAYPILOT_DENY_TERMS`; owner recorded "no private terms" (2026-11-19), so it may be empty and the generic scans still run |
| Identity cutoff (C10.5) | from the first commit |
