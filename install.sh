#!/usr/bin/env bash
set -euo pipefail

cargo install --path .

if command -v OpenTracker >/dev/null 2>&1; then
  OPEN_TRACKER_BIN="$(command -v OpenTracker)"
elif [ -x "$HOME/.cargo/bin/OpenTracker" ]; then
  OPEN_TRACKER_BIN="$HOME/.cargo/bin/OpenTracker"
else
  echo "OpenTracker binary was installed but is not on PATH."
  echo "Use: export PATH=\"\$HOME/.cargo/bin:\$PATH\""
  exit 1
fi

"$OPEN_TRACKER_BIN" onboard --install-daemon
