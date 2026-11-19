#!/bin/sh
# Runs the playpilot guard PreToolUse hook. Fails closed: if no guard can run,
# the tool call is blocked (exit 2).
set -u
root="${CLAUDE_PROJECT_DIR:-$(pwd)}"
if command -v playpilot >/dev/null 2>&1; then
  exec playpilot guard
fi
for bin in "$root/target/release/playpilot" "$root/target/release/playpilot.exe"; do
  [ -x "$bin" ] && exec "$bin" guard
done
if command -v cargo >/dev/null 2>&1 && [ -f "$root/crates/playpilot/Cargo.toml" ]; then
  exec cargo run --quiet --release --locked --manifest-path "$root/Cargo.toml" -p playpilot -- guard
fi
echo "playpilot guard is not installed; run the runtime bootstrap (cargo install --locked --path crates/playpilot)" >&2
exit 2
