#!/bin/sh
# One headless job: clone the repo named by the dispatcher, run one prompt, exit.
# Credentials are injected by the Worker's outbound handler, never baked in.
set -eu
: "${PLAYPILOT_REPO:?}" "${PLAYPILOT_PROMPT:?}"
git clone --depth 50 "https://github.com/$PLAYPILOT_REPO" work
cd work
exec claude -p "$PLAYPILOT_PROMPT" --permission-mode auto --settings "$HOME/.config/fleet/node.settings.json"
