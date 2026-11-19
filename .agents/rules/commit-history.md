# Commit history (canon)

Goal: the base branch reads as a list of finished, reviewable changes. Every
commit on it builds, passes the gates, and says what changed and why.

## H1 Merge method is one value per repository
- Each repository allows exactly one merge method (project profile). The ruleset
  `main-protected` lists only that method.
- New repositories (playpilot, its instance configuration repositories and later ones) use **squash**, with
  `required_linear_history` on the base branch.
- Repositories whose records cite topic-branch commit hashes keep **merge
  commit** (kinekino, humanymous). Their topic branches must then be clean
  themselves (H3).

## H2 Squash commit text comes from the pull request
- The pull request title is the commit subject: Conventional Commits,
  `type(scope): summary`, imperative, at most 72 characters. GitHub appends `(#n)`.
- Repository setting: squash commit title = pull request title, squash commit
  message = blank. The pull request body (provider, session link, evidence)
  stays on the pull request and never enters history.
- A pull request whose title is not a valid subject does not merge (H6).

## H3 What a topic branch may contain before it merges
- One work package per pull request; each commit one logical change.
- No merge commits inside the branch. To catch up with the base, rebase.
  "Update branch" uses the rebase option, never the merge option.
- No `WIP`, `fixup!`, `squash!`, `amend!` or empty commits at merge time.
- Generated churn (lockfiles, regenerated files) goes in its own commit with
  `chore(deps)` or `build` type, never mixed into a feature commit.
- Commit bodies explain why when the subject cannot. No tool or session
  trailers (C10.2, C10.3); author and committer are the owner noreply identity (C10.5).

## H4 Who may rewrite what
- The base branch and tags are never rewritten, by anyone.
- An agent may rewrite **its own unmerged topic branch** (provider prefix,
  only its own commits) to satisfy H3, and push it with
  `--force-with-lease=<branch>:<expected-sha>`. Plain `--force`, `+refspec`
  and rewriting anyone else's branch stay forbidden. This narrows common rule
  C1.3 to: no rewriting of merged, shared or base history.
- After a review comment, prefer a new commit; rewrite only to meet H3 before
  merge, and say so in the pull request.

## H5 Undoing and branches
- Undo a merged change with `git revert` in a new pull request titled
  `revert: <original subject>`.
- Merged head branches are deleted by the repository setting. An agent branch
  with no pull request for 14 days is reported in an issue; the owner deletes it.

## H6 Enforcement
| Where | What |
| --- | --- |
| `playpilot guard` (hook) | refuses force pushes except `--force-with-lease` to a provider-prefixed branch; refuses pushes to protected branches, tags and history rewriting tools |
| `History` required check (`playpilot history-check --range base..head --title "<PR title>"`) | Conventional subject for the PR title (squash) or every commit (merge commit); no merge commits in the range; no WIP/fixup/squash/amend/empty commits; author and committer noreply; no forbidden trailers |
| Ruleset `main-protected` | one merge method; `required_linear_history` for squash repositories; no force push; no deletion |
| Repository settings (files under `.github/settings/`) | squash title/message as in H2; "Always suggest updating pull request branches"; automatic head branch deletion |
