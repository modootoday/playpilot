#!/usr/bin/env bash
# Cloud environment setup script (runs as root before the session starts).
# Installs playpilot guard so the committed .claude/settings.json hook can run.
# Starts in the directory that holds the repository checkouts.
set -euo pipefail
for d in */; do
  if [ -f "$d/crates/playpilot/Cargo.toml" ]; then
    cargo install --locked --root /usr/local --path "$d/crates/playpilot"
    exit 0
  fi
done
cargo install --locked --root /usr/local --git https://github.com/modootoday/playpilot playpilot guard
