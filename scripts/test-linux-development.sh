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
images=("$repo/artifacts/development/linux-x64/"*.AppImage)
[[ ${#images[@]} == 1 ]] || { echo 'Expected exactly one development AppImage'; exit 1; }
appimage="${images[0]}"
# Verify the packaged runtime itself, not just the bundler's input file. The
# only mutable runtime bytes in this unsigned development package are its MD5
# payload digest; an unexpected runtime or update/signature data must fail.
python3 - "$appimage" "$repo/.build/development/bundler-inputs/runtime-x86_64" "$repo/build/development/bundler-inputs.json" > "$work/appimage-runtime.txt" <<'PY_RUNTIME'
import hashlib, json, pathlib, struct, sys
expected = pathlib.Path(sys.argv[2]).read_bytes()
pin = next(i for i in json.load(open(sys.argv[3]))['inputs'] if i['file'] == 'runtime-x86_64')
assert hashlib.sha256(expected).hexdigest() == pin['sha256'], 'Runtime input checksum changed'
assert expected[:6] == b'\x7fELF\x02\x01', 'Expected a little-endian ELF64 runtime'
with open(sys.argv[1], 'rb') as source:
    actual = bytearray(source.read(len(expected)))
expected = bytearray(expected)
header = struct.unpack_from('<16sHHIQQQIHHHHHH', expected)
sections = [struct.unpack_from('<IIQQQQIIQQ', expected, header[6] + i*header[11]) for i in range(header[12])]
strings = sections[header[13]]
names = expected[strings[4]:strings[4]+strings[5]]
digests = [s for s in sections if names[s[0]:].split(b'\0')[0] == b'.digest_md5']
assert len(digests) == 1 and digests[0][5] == 16, 'Unexpected AppImage digest layout'
offset = digests[0][4]
assert len(actual) == len(expected) and offset+16 <= len(expected), 'Truncated AppImage runtime'
actual[offset:offset+16] = expected[offset:offset+16] = bytes(16)
assert actual == expected, 'Packaged AppImage does not embed the exact pinned runtime'
print(json.dumps({'runtime_sha256':pin['sha256'],'runtime_bytes':len(expected),'matched':True,'variable_section':'.digest_md5'}))
PY_RUNTIME
mkdir -p "$work/appimage-extract"
(cd "$work/appimage-extract" && "$appimage" --appimage-extract) > "$work/appimage-extract.log" 2>&1
appdir="$work/appimage-extract/squashfs-root"
[[ -x "$appdir/AppRun" && -x "$appdir/usr/bin/matra-development020" && -x "$appdir/usr/bin/matra-development020-hook" ]]
python3 - "$appdir" > "$work/appimage-contents.txt" <<'PY_CONTENTS'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
desktops = list((root/'usr/share/applications').glob('*.desktop'))
assert any('Exec=matra-development020' in p.read_text() for p in desktops), 'Development AppImage desktop entry is missing'
for item in sorted(root.rglob('*')):
    print(item.relative_to(root))
PY_CONTENTS
exe="$work/root/usr/bin/matra-development020"
hook="$work/root/usr/bin/matra-development020-hook"
[[ -x "$exe" && -x "$hook" ]]
ldd "$exe" > "$work/linked-libraries.txt"
! grep -F 'not found' "$work/linked-libraries.txt"
export MATRA_PREVIEW=1 MATRA_DATA_DIR="$work/profile"
export XDG_SESSION_TYPE=x11 GDK_BACKEND=x11
# Xvfb has no physical GPU. Keep software rendering explicit in this fixture,
# without changing the application's desktop defaults or disabling WebKit.
export LIBGL_ALWAYS_SOFTWARE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1
unset WAYLAND_DISPLAY
printf '%s\n' '{"theme":"light","notch_edge":"left","scale":0.8,"glm_notch_fixed":true,"notch_on_hover":false}' > "$work/profile/config.json"
app='' wm=''
cleanup(){
  local status=$?
  set +e
  [[ ! -f "$work/profile/run.log" ]] || cp "$work/profile/run.log" "$work/run.log"
  ps -eo pid,ppid,stat,comm,args > "$work/processes.log"
  xdotool search --onlyvisible --name '.*' > "$work/visible-windows.txt" 2>&1
  if (( status != 0 )); then
    import -window root "$work/screenshots/x11-failure.png" 2>> "$work/capture-errors.log"
    for log in run.log stderr.log relaunch-stderr.log appimage-stderr.log process-exit.txt; do
      [[ ! -f "$work/$log" ]] || { printf '\nDiagnostic: %s\n' "$log"; tail -100 "$work/$log"; }
    done
  fi
  [[ -z "$app" ]] || kill "$app" 2>/dev/null
  [[ -z "$wm" ]] || kill "$wm" 2>/dev/null
  return "$status"
}
trap cleanup EXIT
check_app(){
  if kill -0 "$app" 2>/dev/null; then return; fi
  local status
  if wait "$app"; then status=0; else status=$?; fi
  printf 'PID=%s exit=%s phase=%s\n' "$app" "$status" "$phase" | tee -a "$work/process-exit.txt"
  if command -v coredumpctl >/dev/null; then
    timeout 5 coredumpctl --no-pager info "$app" > "$work/core-info.log" 2>&1 || true
  fi
  echo 'Native application exited before its lifecycle checks completed' >&2
  return 1
}
# Openbox's startup command runs after it owns the display. Launching GTK while
# the WM is still starting can invalidate a just-discovered native window.
export MATRA_WM_READY="$work/wm-ready-$$"
# Openbox parses argv and spawns directly; it does not interpret redirections.
openbox --sm-disable --startup "/bin/sh -c 'printf ready > \"\$MATRA_WM_READY\"'" > "$work/openbox.log" 2>&1 & wm=$!
for i in $(seq 1 100); do
  kill -0 "$wm"
  [[ -f "$MATRA_WM_READY" ]] && break
  sleep 0.1
done
[[ -f "$MATRA_WM_READY" ]] || { echo 'Openbox did not become ready'; exit 1; }
printf '%s\n' "GDK_BACKEND=$GDK_BACKEND" "LIBGL_ALWAYS_SOFTWARE=$LIBGL_ALWAYS_SOFTWARE" "WEBKIT_DISABLE_DMABUF_RENDERER=$WEBKIT_DISABLE_DMABUF_RENDERER" > "$work/renderer-environment.txt"
wait_settings(){
  local window='' previous='' stable=0 candidate deviation width height crop_x crop_y crop_w crop_h
  for i in $(seq 1 120); do
    check_app
    window=''
    while IFS= read -r candidate; do
      if [[ "$(xdotool getwindowpid "$candidate" 2>/dev/null || true)" == "$app" ]] &&
         xdotool getwindowgeometry --shell "$candidate" > "$work/$phase-geometry.txt" 2>> "$work/window-probes.log"; then
        window="$candidate"
        break
      fi
    done < <(xdotool search --all --onlyvisible --pid "$app" --name 'Settings' 2>/dev/null || true)
    if [[ -n "$window" && "$window" == "$previous" ]]; then stable=$((stable+1)); else stable=0; fi
    previous="$window"
    if (( stable >= 4 )); then
      # A native XID alone does not prove a rendered WebKit surface. Capture the
      # actual application window and reject uniform/black content explicitly.
      # Exclude title/header, borders and navigation: their chrome can paint
      # while the settings pane itself is still blank.
      if import -window "$window" "$work/screenshots/x11-$phase-window.png" 2>> "$work/capture-errors.log"; then
        read -r width height < <(identify -format '%w %h\n' "$work/screenshots/x11-$phase-window.png")
        crop_x=$((width*28/100)); crop_y=$((height*22/100))
        crop_w=$((width*66/100)); crop_h=$((height*68/100))
        if (( crop_w < 32 || crop_h < 32 )); then echo 'Settings content capture is too small' >&2; return 1; fi
        convert "$work/screenshots/x11-$phase-window.png" -crop "${crop_w}x${crop_h}+${crop_x}+${crop_y}" +repage "$work/screenshots/x11-$phase-content.png"
        deviation="$(identify -format '%[fx:standard_deviation]' "$work/screenshots/x11-$phase-content.png")"
        if python3 - "$deviation" <<'PY_PAINT'
import sys
sys.exit(0 if float(sys.argv[1]) > 0.01 else 1)
PY_PAINT
        then
          check_app
          xdotool getwindowgeometry --shell "$window" > "$work/$phase-geometry.txt"
          import -window root "$work/screenshots/x11-development-$phase.png"
          printf 'PID=%s XID=%s content_crop=%sx%s+%s+%s content_standard_deviation=%s\n' "$app" "$window" "$crop_w" "$crop_h" "$crop_x" "$crop_y" "$deviation" > "$work/$phase-paint.txt"
          return
        fi
      fi
    fi
    sleep 0.5
  done
  echo "Native GTK/WebKit settings did not remain visible and paint ($phase)" >&2
  return 1
}
phase=settings
"$exe" --silent > "$work/stdout.log" 2> "$work/stderr.log" & app=$!
wait_settings
python3 - "$work/profile/config.json" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]));assert p['theme']=='light' and p['notch_edge']=='left' and p['scale']==0.8
assert p['port']==48720 and p['automatic_updates'] is False
PY
kill "$app"
wait "$app" || true
app=''
phase=relaunch
"$exe" --silent > "$work/relaunch-stdout.log" 2> "$work/relaunch-stderr.log" & app=$!
wait_settings
python3 - "$work/profile/config.json" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]));assert p['theme']=='light' and p['notch_edge']=='left' and p['scale']==0.8
PY
kill "$app"
wait "$app" || true
app=''
phase=appimage
(cd "$appdir" && exec ./AppRun --silent) > "$work/appimage-stdout.log" 2> "$work/appimage-stderr.log" & app=$!
wait_settings
python3 - "$work/profile/config.json" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]));assert p['theme']=='light' and p['notch_edge']=='left' and p['scale']==0.8
assert p['port']==48720 and p['automatic_updates'] is False
PY
# The fixture helper must not send events or launch an ordinary profile.
"$hook" ping </dev/null
"$appdir/usr/bin/matra-development020-hook" ping </dev/null
printf '%s\n' 'PASS: Debian and AppImage contents, exact embedded runtime, linked libraries, native X11/WebKit startup/content captures and saved-settings relaunch; AppRun preview launched without a host install.' > "$work/RESULT.txt"
