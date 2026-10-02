#!/bin/bash
set -euo pipefail
preview_root="$(cd -- "$(dirname -- "$0")" && pwd)"
preview_binary="$preview_root/native/macos/build/Build/Products/Debug/Matra.app/Contents/MacOS/Matra"
if [[ ! -x "$preview_binary" ]]; then
    printf '%s\n' 'Build the local Mac app first. See native/macos/DASHBOARD-PREVIEW.md.'
    exit 1
fi
# Explicit QA mode bypasses real providers, account discovery, login items,
# update startup and retirement of the installed app. Never omit this flag.
export MATRA_DESIGN_PREVIEW=1
exec "$preview_binary"
