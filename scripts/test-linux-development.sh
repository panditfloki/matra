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
seed_config='{"theme":"light","notch_edge":"left","scale":0.8,"glm_notch_fixed":true,"notch_on_hover":false}'
printf '%s\n' "$seed_config" > "$work/profile/config.json"
# app is always the real application PID. tracer is set only during the post-failure
# diagnostic reproductions, where the app is the debugger's child and not this shell's.
app='' wm='' tracer='' phase=''
# kill 0/-1 and negative PIDs address process groups (this CI shell's included). Only a
# plain decimal PID above 1 that is not this shell may ever be signalled or polled.
valid_pid(){ [[ "${1-}" =~ ^[1-9][0-9]{0,9}$ ]] && (( $1 > 1 && $1 != $$ )); }
stop_app(){
  [[ -n "$app" ]] || return 0
  valid_pid "$app" || { app=''; return 0; }
  kill "$app" 2>/dev/null
  for i in $(seq 1 50); do kill -0 "$app" 2>/dev/null || break; sleep 0.1; done
  kill -KILL "$app" 2>/dev/null
  [[ -n "$tracer" ]] || wait "$app" 2>/dev/null
  app=''
}
cleanup(){
  local status=$?
  set +e
  [[ ! -f "$work/profile/run.log" ]] || cp "$work/profile/run.log" "$work/run.log"
  ps -eo pid,ppid,stat,comm,args > "$work/processes.log"
  xdotool search --onlyvisible --name '.*' > "$work/visible-windows.txt" 2>&1
  if (( status != 0 )); then
    import -window root "$work/screenshots/x11-failure.png" 2>> "$work/capture-errors.log"
    for log in run.log stdout.log stderr.log relaunch-stdout.log relaunch-stderr.log appimage-stdout.log appimage-stderr.log process-exit.txt; do
      [[ ! -f "$work/$log" ]] || { printf '\nDiagnostic: %s\n' "$log"; tail -100 "$work/$log"; }
    done
  fi
  stop_app
  # The first failure above is already recorded. Diagnostics run afterwards in their own
  # profile and directory and never change this job's status.
  if (( status != 0 )) && [[ -n "$phase" && "${MATRA_NATIVE_TRACE:-1}" != 0 ]]; then
    native_trace
  fi
  [[ -z "$wm" ]] || kill "$wm" 2>/dev/null
  return "$status"
}
trap cleanup EXIT
check_app(){
  if ! valid_pid "$app"; then
    printf 'Invalid application PID %q phase=%s; nothing signalled or polled\n' "$app" "$phase" >&2
    return 1
  fi
  # Explicit statuses only: these functions also run from the EXIT trap (native_trace), where
  # bash 5.1's bare `return` yields the status saved before the trap (the original failure, 1).
  # That made the live traced app look dead at once and get signalled within its loader.
  if kill -0 "$app" 2>/dev/null; then return 0; fi
  local status
  if [[ -n "$tracer" ]]; then
    # Diagnostic reproduction: the tracer reaped the app; it records the status itself.
    printf 'PID=%s ended phase=%s (status in the tracer record)\n' "$app" "$phase" | tee -a "$work/native-trace/process-exit.txt"
    return 1
  fi
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
    check_app || return 1
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
          check_app || return 1
          xdotool getwindowgeometry --shell "$window" > "$work/$phase-geometry.txt"
          import -window root "$work/screenshots/x11-development-$phase.png"
          printf 'PID=%s XID=%s content_crop=%sx%s+%s+%s content_standard_deviation=%s\n' "$app" "$window" "$crop_w" "$crop_h" "$crop_x" "$crop_y" "$deviation" > "$work/$phase-paint.txt"
          return 0
        fi
      fi
    fi
    sleep 0.5
  done
  echo "Native GTK/WebKit settings did not remain visible and paint ($phase)" >&2
  return 1
}
# X11 connection reuse. The 20 Hz pointer watchdog samples through the X11 carry adapter for the
# whole run; every sample must use GTK's own (borrowed) X connection. Tauri's Linux display_handle()
# opened a new, never-closed X client per call, so socket descriptors grew ~20 per second. Count the
# app's sockets across ~60 further samples and require exactly one borrowed-connection log line.
socket_fds(){ local n; n="$(find "$1" -mindepth 1 -maxdepth 1 -lname 'socket:*' 2>/dev/null | wc -l)"; echo $((n)); return 0; }
check_x11_connections(){
  local log="$work/profile/run.log" before after borrowed unavailable
  check_app || return 1
  before="$(socket_fds "/proc/$app/fd")"
  sleep 3
  check_app || return 1
  after="$(socket_fds "/proc/$app/fd")"
  borrowed="$(grep -c "carry: X11 pointer uses GTK's display connection" "$log" 2>/dev/null || true)"
  unavailable="$(grep 'watchdog: pointer sample unavailable' "$log" 2>/dev/null || true)"
  printf 'PID=%s socket_fds_before=%s socket_fds_after=%s borrowed_connection_lines=%s sample_unavailable=%s\n' \
    "$app" "$before" "$after" "${borrowed:-missing}" "${unavailable:-none}" | tee "$work/x11-connections.txt"
  if (( before < 1 || after - before > 8 )); then
    echo 'X11 client sockets grew while the pointer watchdog sampled (per-sample connection leak)' >&2
    return 1
  fi
  if [[ "$borrowed" != 1 || -n "$unavailable" ]]; then
    echo 'Pointer sampling did not run on exactly one borrowed GTK X11 connection' >&2
    return 1
  fi
  return 0
}
# DIAGNOSTIC ONLY, after an application phase has already failed. Two isolated, time-bounded
# reproductions of the extracted Debian binary in fresh fixture profiles (preview mode, so no
# provider, hook, credential or update service starts): gdb records native stacks at
# termination and X11 connection-failure entry points; strace records the syscall timeline
# including X11 socket I/O. The same wait_settings gate observes the real application PID.
# A traced pass is reported as such and never replaces the failure above.
native_trace(){
  local trace="$work/native-trace" failed_phase="$phase" outcome candidate
  mkdir -p "$trace/gdb-profile" "$trace/strace-profile"
  {
    echo "DIAGNOSTIC ONLY: traced reproductions after phase '$failed_phase' failed. Not acceptance evidence; the job keeps its original failure."
    echo "binary=$exe"
    command -v gdb && gdb --version | head -1
    command -v strace && strace -V | head -1
  } > "$trace/README.txt" 2>&1
  if command -v gdb >/dev/null; then
    printf '%s\n' "$seed_config" > "$trace/gdb-profile/config.json"
    MATRA_DATA_DIR="$trace/gdb-profile" MATRA_TRACE_DIR="$trace" \
      timeout --kill-after=10 150 gdb -nx -q -batch -x "$repo/scripts/linux-native-trace-gdb.py" --args "$exe" --silent \
      < /dev/null > "$trace/gdb.log" 2>&1 & tracer=$!
    for i in $(seq 1 300); do [[ -s "$trace/gdb-app.pid" ]] && break; kill -0 "$tracer" 2>/dev/null || break; sleep 0.1; done
    candidate=''
    [[ ! -s "$trace/gdb-app.pid" ]] || IFS= read -r candidate < "$trace/gdb-app.pid" || true
    if valid_pid "$candidate"; then
      app="$candidate" phase=diagnostic-gdb
      if wait_settings; then outcome='traced app stayed up and painted Settings'; else outcome='traced app ended or did not paint'; fi
      # A still-running app gets SIGSTOP through ptrace: gdb then dumps every thread and ends it.
      valid_pid "$app" && kill -0 "$app" 2>/dev/null && kill -STOP "$app" 2>/dev/null
    else
      outcome="gdb did not start the application (pid file: '${candidate}')"
      # No application to observe: end the timeout wrapper now instead of waiting it out.
      valid_pid "$tracer" && kill -0 "$tracer" 2>/dev/null && kill -TERM "$tracer" 2>/dev/null
    fi
    wait "$tracer"; printf '%s; gdb wrapper status=%s\n' "$outcome" "$?" > "$trace/gdb-outcome.txt"
    tracer='' app=''
    # Build IDs of every object named in a stack, for offline symbolisation by build ID.
    grep -o 'from /[^ ]*' "$trace/gdb-stops.txt" 2>/dev/null | sort -u | cut -c6- | while read -r object; do
      printf '%s ' "$object"; readelf -n "$object" 2>/dev/null | awk '/Build ID/ {print $3}'
    done > "$trace/build-ids.txt"
    printf '%s ' "$exe" >> "$trace/build-ids.txt"; readelf -n "$exe" 2>/dev/null | awk '/Build ID/ {print $3}' >> "$trace/build-ids.txt"
  else
    echo 'gdb is not installed on this runner' > "$trace/gdb-outcome.txt"
  fi
  if command -v strace >/dev/null; then
    printf '%s\n' "$seed_config" > "$trace/strace-profile/config.json"
    MATRA_DATA_DIR="$trace/strace-profile" \
      timeout --kill-after=10 150 strace -f -tt -T -yy -s 96 -e 'trace=!futex' -o "$trace/strace.txt" "$exe" --silent \
      < /dev/null > "$trace/strace-app-stdout.log" 2> "$trace/strace-app-stderr.log" & tracer=$!
    for i in $(seq 1 300); do [[ -s "$trace/strace.txt" ]] && break; kill -0 "$tracer" 2>/dev/null || break; sleep 0.1; done
    candidate="$(awk 'NR==1 {print $1; exit}' "$trace/strace.txt" 2>/dev/null)"
    if valid_pid "$candidate"; then
      app="$candidate" phase=diagnostic-strace
      if wait_settings; then outcome='traced app stayed up and painted Settings'; else outcome='traced app ended or did not paint'; fi
      valid_pid "$app" && kill -0 "$app" 2>/dev/null && kill -TERM "$app" 2>/dev/null
    else
      outcome="strace did not start the application (first field: '${candidate}')"
      valid_pid "$tracer" && kill -0 "$tracer" 2>/dev/null && kill -TERM "$tracer" 2>/dev/null
    fi
    wait "$tracer"; printf '%s; strace wrapper status=%s\n' "$outcome" "$?" > "$trace/strace-outcome.txt"
    tracer='' app=''
    [[ ! -f "$trace/strace.txt" ]] || python3 -I "$repo/scripts/linux-native-trace-summary.py" "$trace/strace.txt" "$trace/strace-summary.txt" 2>> "$trace/strace-outcome.txt"
  else
    echo 'strace is not installed on this runner' > "$trace/strace-outcome.txt"
  fi
  phase="$failed_phase"
  for log in gdb-outcome.txt strace-outcome.txt gdb-exit.json; do
    [[ ! -f "$trace/$log" ]] || { printf '\nDiagnostic trace: %s\n' "$log"; cat "$trace/$log"; }
  done
  [[ ! -f "$trace/gdb-stops.txt" ]] || { printf '\nDiagnostic trace: first full stop\n'; awk '/FULL STOP #1:/,/--- shared libraries/' "$trace/gdb-stops.txt" | head -150; }
  [[ ! -f "$trace/strace-summary.txt" ]] || { printf '\nDiagnostic trace: strace summary head\n'; head -60 "$trace/strace-summary.txt"; }
}
phase=settings
"$exe" --silent > "$work/stdout.log" 2> "$work/stderr.log" & app=$!
wait_settings
check_x11_connections
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
