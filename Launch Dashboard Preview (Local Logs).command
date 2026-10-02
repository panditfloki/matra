#!/bin/bash
set -euo pipefail
preview_root="$(cd -- "$(dirname -- "$0")" && pwd)"
preview_app="$preview_root/native/macos/build/Build/Products/Debug/Matra.app"
preview_binary="$preview_app/Contents/MacOS/Matra"
if [[ ! -x "$preview_binary" ]]; then
    printf '%s\n' 'Build the local Mac app first. See native/macos/DASHBOARD-PREVIEW.md.'
    exit 1
fi
# make test and make build leave the installed app's identity in this folder.
# Only the separate preview identity may run beside the installed app.
bundle_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$preview_app/Contents/Info.plist" 2>/dev/null || true)"
if [[ "$bundle_id" != "com.dydxfx.matra.mac.preview" ]]; then
    printf '%s\n' "This build is '${bundle_id:-unknown}', not the preview. Run 'make preview-build' in native/macos first."
    exit 1
fi
# Real local logs through the offline reader. Quota polling, account
# discovery, login items, update startup and retirement of the installed
# app stay off. Never omit these flags.
export MATRA_DESIGN_PREVIEW=1
export MATRA_PREVIEW_DATA=local
exec "$preview_binary"
