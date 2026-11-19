#!/bin/sh
# Runs `playpilot guard` as a PreToolUse hook. Used by the plugin and by the
# playpilot repository's own .claude/settings.json. Fails closed: when no guard
# can run, the tool call is blocked (exit 2).
set -u
if command -v playpilot >/dev/null 2>&1; then
  exec playpilot guard
fi
root="${CLAUDE_PROJECT_DIR:-$(pwd)}"
for bin in "$root/target/release/playpilot" "$root/target/release/playpilot.exe"; do
  [ -x "$bin" ] && exec "$bin" guard
done
if command -v cargo >/dev/null 2>&1 && [ -f "$root/crates/playpilot/Cargo.toml" ]; then
  exec cargo run --quiet --release --locked --manifest-path "$root/Cargo.toml" -p playpilot -- guard
fi
echo "playpilot is not installed; run /playpilot:install (or: cargo install --locked --git https://github.com/modootoday/playpilot playpilot)" >&2
exit 2
