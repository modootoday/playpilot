#!/usr/bin/env bash
# Prepares a WSL distribution as a fleet node. Idempotent. Run as the node user (not root).
set -euo pipefail
[ "$(id -u)" -ne 0 ] || { echo "run as the node user, not root" >&2; exit 1; }
repo="$(cd "$(dirname "$0")/../.." && pwd)"
case "$repo" in /mnt/*) echo "clone the starter onto the Linux filesystem (~), not /mnt/c" >&2; exit 1;; esac

command -v claude >/dev/null || curl -fsSL https://claude.ai/install.sh | bash
command -v gh >/dev/null || { echo "install gh (apt) and run: gh auth login" >&2; exit 1; }
command -v cargo >/dev/null || { echo "install rustup first" >&2; exit 1; }
command -v cc >/dev/null || { echo "install a C linker first (Ubuntu: sudo apt install -y build-essential)" >&2; exit 1; }

cargo install --locked --path "$repo/crates/playpilot"
mkdir -p ~/.config/fleet ~/.config/systemd/user
install -m 0644 "$repo/profiles/node.settings.json" ~/.config/fleet/node.settings.json
install -m 0644 "$repo/runtimes/wsl/systemd/claude-rc@.service" ~/.config/systemd/user/
systemctl --user daemon-reload
loginctl enable-linger "$USER" 2>/dev/null || echo "note: run 'sudo loginctl enable-linger $USER' so the node runs without a login shell"
cat <<MSG
Next (owner, once, in a terminal: the prompts need a TTY that systemd does not give):
  1. claude auth login                        # full-scope login; setup-token cannot serve Remote Control
  2. cd ~/fleet/<dir> && claude               # accept workspace trust and the first-run wizard, then /exit
  3. claude remote-control                    # answer y to "Enable Remote Control?" once, then Ctrl+C
  4. systemctl --user enable --now claude-rc@<dir>
  5. journalctl --user -u claude-rc@<dir> -n 20   # confirm it connected (errors only; stdout is discarded)
  6. On Windows: runtimes/windows/wsl-autostart.ps1 -Distro <name>   # start WSL at boot
MSG
