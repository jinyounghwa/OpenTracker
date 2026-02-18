#!/usr/bin/env bash
set -euo pipefail

cargo install --path .
OpenTracker onboard --install-daemon
