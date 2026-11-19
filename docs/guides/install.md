---
---
# Install a node

A node is a machine that runs Claude Code for the fleet: a WSL distribution,
a Windows user account, or a container.

1. Install Claude Code and sign in with `claude auth login`. On WSL also install
   a C linker (`sudo apt install -y build-essential`), rustup and `gh`.
2. Add the plugin:

   ```
   /plugin marketplace add modootoday/playpilot
   /plugin install playpilot@playpilot
   ```

3. Install the CLI the guard runs: `/playpilot:install`, or
   `cargo install --locked --git https://github.com/modootoday/playpilot playpilot`.
4. Run the runtime bootstrap from the repository: `runtimes/wsl/bootstrap.sh`
   on WSL or `runtimes/windows/bootstrap.ps1` on Windows. It installs the node
   profile and the service that keeps the Remote Control server running.
5. Check the guard: asking Claude to push to `main` is refused.

Cloud sessions do not load plugins. They get the same guard from the
`.claude/settings.json` that `playpilot render` writes into each repository.

From the fleet configuration repository (it holds `fleet/` and
`playpilot.lock`), write or check a project's hook:

```
playpilot render settings --project <name> --repo <project checkout>
playpilot render settings --project <name> --repo <project checkout> --check
```

The rendered hook pins the rev from `playpilot.lock`, takes the protected
branches from the `Project` resource, reinstalls the CLI when the recorded rev
differs, and blocks the tool call when no CLI can run. Other settings and
hooks in the file are kept.
