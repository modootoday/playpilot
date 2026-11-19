---
description: Install or update the playpilot CLI that the guard hook runs
---
Install the playpilot command-line tool for this machine so the plugin's guard hook can run.

1. Check whether `playpilot --version` already works. If it does, report the version and stop unless the user asked to update.
2. If `cargo` is available, run `cargo install --locked --git https://github.com/modootoday/playpilot playpilot`. If it is not, tell the user to install Rust with rustup first; do not install toolchains without asking.
3. Run `playpilot --version` and report the result.

Never run this with elevated privileges, and never change the user's shell profile without asking.
