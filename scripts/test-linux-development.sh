#!/usr/bin/env bash
# Run only inside the disposable CI X11 display. No host package installation.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_OS:-}" == Linux ]] || { echo 'Disposable Linux CI only' >&2; exit 2; }
repo="$(cd "$(dirname "$0")/.." && pwd)"
work="$repo/.build/linux-lifecycle"
mkdir -p "$work/root" "$work/profile" "$work/screenshots"
shopt -s nullglob
packages=("$repo/artifacts/development/linux-x64/"*.deb)
[[ ${#packages[@]} == 1 ]] || { echo 'Expected exactly one development Debian package'; exit 1; }
package="${packages[0]}"
[[ -n "$package" ]] || { echo 'Missing development Debian package'; exit 1; }
dpkg-deb --contents "$package" > "$work/deb-contents.txt"
dpkg-deb --extract "$package" "$work/root"
exe="$work/root/usr/bin/matra-development020"
hook="$work/root/usr/bin/matra-development020-hook"
[[ -x "$exe" && -x "$hook" ]]
ldd "$exe" > "$work/linked-libraries.txt"
! grep -F 'not found' "$work/linked-libraries.txt"
export MATRA_PREVIEW=1 MATRA_DATA_DIR="$work/profile"
export XDG_SESSION_TYPE=x11
unset WAYLAND_DISPLAY
printf '%s\n' '{"theme":"light","notch_edge":"left","scale":0.8,"glm_notch_fixed":true,"notch_on_hover":false}' > "$work/profile/config.json"
openbox > "$work/openbox.log" 2>&1 & wm=$!
"$exe" --silent > "$work/stdout.log" 2> "$work/stderr.log" & app=$!
cleanup(){ kill "$app" "$wm" 2>/dev/null || true; }
trap cleanup EXIT
window=''
for i in $(seq 1 90); do
  kill -0 "$app"
  window="$(xdotool search --onlyvisible --name 'Settings' 2>/dev/null | head -1 || true)"
  [[ -n "$window" ]] && break
  sleep 0.5
done
[[ -n "$window" ]] || { echo 'Native GTK/WebKit settings did not appear'; exit 1; }
sleep 2
import -window root "$work/screenshots/x11-development-settings.png"
xdotool getwindowgeometry --shell "$window" > "$work/settings-geometry.txt"
python3 - "$work/profile/config.json" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]));assert p['theme']=='light' and p['notch_edge']=='left' and p['scale']==0.8
assert p['port']==48720 and p['automatic_updates'] is False
PY
kill "$app"
wait "$app" || true
"$exe" --silent > "$work/relaunch-stdout.log" 2> "$work/relaunch-stderr.log" & app=$!
sleep 4
kill -0 "$app"
import -window root "$work/screenshots/x11-development-relaunch.png"
python3 - "$work/profile/config.json" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]));assert p['theme']=='light' and p['notch_edge']=='left' and p['scale']==0.8
PY
# The fixture helper must not send events or launch an ordinary profile.
"$hook" ping </dev/null
printf '%s\n' 'PASS: extracted development package, linked libraries, native X11/WebKit startup, screenshot and saved-settings relaunch.' > "$work/RESULT.txt"
