#!/usr/bin/env python3
"""Physical X11 carry acceptance for the packaged Linux development app.

Runs only inside the disposable GitHub Linux CI X11 display (Xvfb + Openbox), against the real
GTK/WebKit process started by scripts/test-linux-development.sh in MATRA_PREVIEW mode.

Input is real X server input: xdotool moves the core pointer and presses/releases button 1 and
Escape through XTEST, exactly as a device would. Nothing is injected into the page, no IPC is
called and no carry state is simulated. Every observation is native:
  * windows: XIDs owned by the application PID (xdotool), their mapped state and X geometry;
  * pointer grab ownership: a separate short-lived X client asks for the pointer (XGrabPointer on
    the root). AlreadyGrabbed means another client holds it; success is ungrabbed at once;
  * pointer button / Escape state: XQueryPointer / XQueryKeymap from that same probe client;
  * paint: ImageMagick capture of the actual overlay XID;
  * application decisions: the app's own run.log lines (`notch placed`, `click-through`,
    `notch carried to`, `notch carry <id>: <error>`) and the saved config.json.

The grip is located from the app's own hit-test data: the `click-through off` line logs the hot
rectangles (physical pixels, relative to the notch window) that the native watchdog uses to decide
whether the pointer is on the notch. The grip is the first handle rectangle followed by its bridge.

Every wait is bounded. Held input is released in `finally` without replacing the original error,
and the shell harness releases it again from its EXIT trap if this process is killed.
"""
import ctypes
import ctypes.util
import json
import os
import re
import signal
import subprocess
import sys
import time

BUTTON1_MASK = 1 << 8
GRAB_SUCCESS, ALREADY_GRABBED = 0, 1
NOTCH_TITLE = 'dy/dx.f(Mātrā)'
OVERLAY_TITLE = 'Matra carry'
PLACED = re.compile(r'notch placed build=.*?: edge=(\w+) pos=\((-?\d+),(-?\d+)\) size=\((\d+)x(\d+)\)'
                    r'.*? work=\((-?\d+), (-?\d+), (-?\d+), (-?\d+)\)')
CLICK_OFF = re.compile(r'^click-through off \(cursor on the notch\) at cursor_rel=\((-?\d+),(-?\d+)\) rects=(\[.*\])\s*$')
CLICK_ON = re.compile(r'^click-through on \(cursor elsewhere\)')
CARRIED = re.compile(r'^notch carried to (\w+) at (-?[0-9.]+) on (.*)$')
CARRY_ERROR = re.compile(r'^notch carry (\d+): (.*)$')


class Failure(Exception):
    pass


# ---------------------------------------------------------------------------------------------
# Pure helpers (covered by test/linux-native-carry.test.js on any host)
# ---------------------------------------------------------------------------------------------
def parse_rects(text):
    """Rust `{:?}` of Vec<[f64; 4]> -> list of [x, y, w, h] floats. Rejects anything else."""
    value = json.loads(text)
    if not isinstance(value, list) or not all(
            isinstance(r, list) and len(r) == 4 and all(isinstance(v, (int, float)) for v in r) for r in value):
        raise Failure(f'Unexpected hot rectangle list: {text[:200]}')
    return [[float(v) for v in r] for r in value]


def vertical(edge):
    return edge in ('left', 'right')


def handle_pairs(rects, edge):
    """(index of handle, index of its bridge) in reported order. notch.html reports the pill (and any
    card/alert surfaces) first, then for each handle its circle followed by a bridge exactly as wide
    as the circle across the edge axis: the grip first, then the settings orb."""
    pairs = []
    i = 0
    while i + 1 < len(rects):
        r, b = rects[i], rects[i + 1]
        square = r[2] > 4 and abs(r[2] - r[3]) <= 1.5
        if vertical(edge):
            bridge = abs(b[0] - r[0]) <= 0.5 and abs(b[2] - r[2]) <= 0.5
        else:
            bridge = abs(b[1] - r[1]) <= 0.5 and abs(b[3] - r[3]) <= 0.5
        if i > 0 and square and bridge:
            pairs.append((i, i + 1))
            i += 2
        else:
            i += 1
    return pairs


def grip_rect(rects, edge):
    pairs = handle_pairs(rects, edge)
    if len(pairs) != 2:
        raise Failure(f'Expected the grip and settings handles (2 circle+bridge pairs) in the hot rectangles; '
                      f'found {len(pairs)}: {rects}')
    grip, orb = rects[pairs[0][0]], rects[pairs[1][0]]
    axis = 1 if vertical(edge) else 0
    if not grip[axis] > orb[axis]:
        raise Failure(f'The grip is not beyond the settings orb along the {edge} edge: grip={grip} orb={orb}')
    return grip


def center(rect):
    return rect[0] + rect[2] / 2, rect[1] + rect[3] / 2


def placement(config):
    return {'edge': config.get('notch_edge'), 'along': config.get('notch_along'), 'monitor': config.get('notch_monitor')}


def monitor_matches(logged, saved):
    """`{:?}` of Option<String> in the log against the saved notch_monitor."""
    if logged == 'None':
        return saved is None
    match = re.fullmatch(r'Some\("(.*)"\)', logged)
    return bool(match) and saved == match.group(1)


def same_rect(a, b, tolerance=1):
    return all(abs(x - y) <= tolerance for x, y in zip(a, b))


# ---------------------------------------------------------------------------------------------
# Native observation
# ---------------------------------------------------------------------------------------------
class X11Probe:
    """A separate X client used only to observe; it never sends input."""

    def __init__(self):
        name = ctypes.util.find_library('X11') or 'libX11.so.6'
        x = ctypes.CDLL(name)
        x.XOpenDisplay.restype = ctypes.c_void_p
        x.XOpenDisplay.argtypes = [ctypes.c_char_p]
        x.XCloseDisplay.argtypes = [ctypes.c_void_p]
        x.XDefaultRootWindow.restype = ctypes.c_ulong
        x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
        x.XGrabPointer.restype = ctypes.c_int
        x.XGrabPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_uint, ctypes.c_int,
                                   ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong]
        x.XUngrabPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        x.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
        u = ctypes.POINTER(ctypes.c_ulong)
        i = ctypes.POINTER(ctypes.c_int)
        x.XQueryPointer.restype = ctypes.c_int
        x.XQueryPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong, u, u, i, i, i, i, ctypes.POINTER(ctypes.c_uint)]
        x.XQueryKeymap.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        x.XKeysymToKeycode.restype = ctypes.c_ubyte
        x.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x = x

    def _with(self, use):
        display = self.x.XOpenDisplay(None)
        if not display:
            raise Failure('The X11 observation client could not connect to $DISPLAY')
        try:
            return use(display, self.x.XDefaultRootWindow(display))
        finally:
            self.x.XCloseDisplay(display)

    def grab_state(self):
        """'held' when another client owns the pointer, 'free' when it could be grabbed (released at once)."""
        def probe(d, root):
            status = self.x.XGrabPointer(d, root, 0, (1 << 2) | (1 << 3), 1, 1, 0, 0, 0)
            if status == GRAB_SUCCESS:
                self.x.XUngrabPointer(d, 0)
            self.x.XSync(d, 0)
            return status
        status = self._with(probe)
        if status == GRAB_SUCCESS:
            return 'free'
        if status == ALREADY_GRABBED:
            return 'held'
        raise Failure(f'Unexpected XGrabPointer probe status {status}')

    def pointer(self):
        def query(d, root):
            r, c = ctypes.c_ulong(), ctypes.c_ulong()
            rx, ry, wx, wy, mask = ctypes.c_int(), ctypes.c_int(), ctypes.c_int(), ctypes.c_int(), ctypes.c_uint()
            if not self.x.XQueryPointer(d, root, ctypes.byref(r), ctypes.byref(c), ctypes.byref(rx), ctypes.byref(ry),
                                        ctypes.byref(wx), ctypes.byref(wy), ctypes.byref(mask)):
                raise Failure('XQueryPointer: the pointer is not on the default screen')
            keys = ctypes.create_string_buffer(32)
            self.x.XQueryKeymap(d, keys)
            code = self.x.XKeysymToKeycode(d, 0xff1b)
            escape = code != 0 and bool(keys.raw[code // 8] & (1 << (code % 8)))
            return {'x': rx.value, 'y': ry.value, 'button1': bool(mask.value & BUTTON1_MASK), 'escape': escape}
        return self._with(query)


def run(command, check=True):
    done = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
    if check and done.returncode != 0:
        raise Failure(f'{command[0]} {command[1] if len(command) > 1 else ""} failed ({done.returncode}): '
                      f'{done.stderr.decode(errors="replace").strip()}')
    return done


def wait_for(predicate, seconds, what, interval=0.05):
    deadline = time.monotonic() + seconds
    while True:
        value = predicate()
        if value:
            return value
        if time.monotonic() >= deadline:
            raise Failure(f'Timed out after {seconds}s waiting for {what}')
        time.sleep(interval)


class Session:
    def __init__(self, pid, work):
        self.pid = pid
        self.work = work
        self.log_path = os.path.join(work, 'profile', 'run.log')
        self.config_path = os.path.join(work, 'profile', 'config.json')
        self.shots = os.path.join(work, 'screenshots')
        self.x11 = X11Probe()
        self.held_button = False
        self.held_escape = False
        self.lines_seen = 0

    # -- application liveness and logs ------------------------------------------------------
    def alive(self):
        try:
            os.kill(self.pid, 0)
        except ProcessLookupError:
            raise Failure(f'Application PID {self.pid} ended during the carry checks')
        return True

    def log_lines(self):
        try:
            with open(self.log_path, encoding='utf-8', errors='replace') as handle:
                return handle.read().splitlines()
        except FileNotFoundError:
            return []

    def mark(self):
        """A position in run.log; later reads see only lines after it. run.log restarts past its
        size cap, which a shorter file reveals; the new file is then read from its start."""
        lines = self.log_lines()
        return len(lines)

    def since(self, mark):
        lines = self.log_lines()
        return lines[mark:] if len(lines) >= mark else lines

    def config(self):
        def read():
            try:
                with open(self.config_path, encoding='utf-8') as handle:
                    return json.load(handle)
            except (OSError, ValueError):
                return None
        return wait_for(read, 3, 'a readable config.json')

    # -- windows ----------------------------------------------------------------------------
    def windows(self, visible=True):
        command = ['xdotool', 'search'] + (['--onlyvisible'] if visible else []) + ['--pid', str(self.pid)]
        done = run(command, check=False)
        found = {}
        for xid in done.stdout.decode().split():
            name = run(['xdotool', 'getwindowname', xid], check=False)
            if name.returncode == 0:
                found[xid] = name.stdout.decode('utf-8', errors='replace').rstrip('\n')
        return found

    def titled(self, title, visible=True):
        return [xid for xid, name in self.windows(visible).items() if name == title]

    def geometry(self, xid):
        values = dict(line.split('=', 1) for line in run(['xdotool', 'getwindowgeometry', '--shell', xid]).stdout.decode().split())
        return [int(values['X']), int(values['Y']), int(values['WIDTH']), int(values['HEIGHT'])]

    def notch(self):
        found = self.titled(NOTCH_TITLE)
        if len(found) != 1:
            raise Failure(f'Expected exactly one visible notch XID owned by PID {self.pid}; found {self.windows()}')
        return found[0]

    def overlays(self):
        # Mapped or not: a destroyed overlay must be gone entirely, and more than one is a defect.
        return self.titled(OVERLAY_TITLE, visible=False)

    def placed(self, mark=0, edge=None):
        for line in reversed(self.since(mark)):
            match = PLACED.search(line)
            if match and (edge is None or match.group(1) == edge):
                values = [int(v) for v in match.groups()[1:]]
                return {'edge': match.group(1), 'rect': values[:4], 'work': values[4:]}
        return None

    def notch_matches_log(self, mark=0, edge=None):
        """The notch XID's X geometry equals the app's own most recent placement record."""
        record = self.placed(mark, edge)
        if not record:
            return None
        xid = self.notch()
        rect = self.geometry(xid)
        return {'xid': xid, 'rect': rect, 'record': record} if same_rect(rect, record['rect'], 2) else None

    def capture(self, xid, name):
        path = os.path.join(self.shots, name)
        run(['import', '-window', xid, path])
        deviation = run(['convert', path, '-alpha', 'off', '-format', '%[fx:standard_deviation]', 'info:']).stdout.decode().strip()
        return path, float(deviation)

    def capture_root(self, name):
        run(['import', '-window', 'root', os.path.join(self.shots, name)], check=False)

    # -- real input -------------------------------------------------------------------------
    def move(self, x, y, settle=0.03):
        self.alive()
        run(['xdotool', 'mousemove', str(int(round(x))), str(int(round(y)))])
        time.sleep(settle)

    def press(self):
        self.held_button = True
        run(['xdotool', 'mousedown', '1'])

    def release(self):
        run(['xdotool', 'mouseup', '1'])
        self.held_button = False

    def escape_down(self):
        self.held_escape = True
        run(['xdotool', 'keydown', 'Escape'])

    def escape_up(self):
        run(['xdotool', 'keyup', 'Escape'])
        self.held_escape = False

    def release_all(self):
        errors = []
        for held, name, command in ((self.held_button, 'left button', ['xdotool', 'mouseup', '1']),
                                    (self.held_escape, 'Escape', ['xdotool', 'keyup', 'Escape'])):
            if held:
                try:
                    run(command)
                except Exception as error:  # every release is attempted
                    errors.append(f'{name}: {error}')
        self.held_button = self.held_escape = False
        return errors


# ---------------------------------------------------------------------------------------------
# Acceptance steps
# ---------------------------------------------------------------------------------------------
def check(condition, message, evidence=None):
    if not condition:
        raise Failure(f'{message}: {json.dumps(evidence, default=str)}' if evidence is not None else message)
    print(f'PASS: {message}', flush=True)


def locate_grip(s, display):
    """Hover the actual notch with the real pointer until the native watchdog records a crossing,
    and take the grip from the hot rectangles it logged."""
    edge = s.config()['notch_edge']
    state = wait_for(lambda: s.notch_matches_log(edge=edge), 10, f'the notch XID to match its logged {edge} placement')
    x, y, w, h = state['rect']
    s.move(display[0] - 4, display[1] - 4, 0.6)  # off the notch: the next hover is a fresh crossing
    mark = s.mark()
    steps = 48
    for k in range(steps + 1):
        t = 4 + (h - 8 if vertical(edge) else w - 8) * k / steps
        if edge == 'left':
            s.move(x + 12, y + t, 0.09)
        elif edge == 'right':
            s.move(x + w - 12, y + t, 0.09)
        elif edge == 'top':
            s.move(x + t, y + 12, 0.09)
        else:
            s.move(x + t, y + h - 12, 0.09)
        for line in s.since(mark):
            match = CLICK_OFF.match(line)
            if match:
                rects = parse_rects(match.group(3))
                grip = grip_rect(rects, edge)
                return {'edge': edge, 'notch': state, 'rects': rects, 'grip': grip,
                        'point': [x + center(grip)[0], y + center(grip)[1]]}
    raise Failure(f'The real pointer crossed the {edge} notch strip without a native click-through change; '
                  f'log since hover: {s.since(mark)[-12:]}')


def arm_grip(s, display, located):
    """Leave, then arrive on the grip so the native hit test hands the window its input back."""
    mark = s.mark()
    s.move(display[0] - 4, display[1] - 4)
    # The hover above left the notch accepting input; moving off must hand clicks back to the desktop.
    wait_for(lambda: any(CLICK_ON.match(l) for l in s.since(mark)), 2,
             'the native watchdog to make the notch click-through again after the pointer left')
    time.sleep(0.8)  # card/hover grace: 300 ms leave + 250 ms page grace, with margin
    mark = s.mark()
    s.move(*located['point'], 0.05)

    def on_grip():
        for line in s.since(mark):
            match = CLICK_OFF.match(line)
            if match:
                return [int(match.group(1)), int(match.group(2))]
        return None
    rel = wait_for(on_grip, 3, 'the native watchdog to report the pointer on the grip')
    g = located['grip']
    check(g[0] - 1 <= rel[0] <= g[0] + g[2] + 1 and g[1] - 1 <= rel[1] <= g[1] + g[3] + 1,
          'Native hit test places the real pointer inside the grip rectangle', {'cursor_rel': rel, 'grip': g})
    time.sleep(0.3)  # the page receives the arrival and reveals the handle
    pointer = s.x11.pointer()
    check(abs(pointer['x'] - located['point'][0]) <= 1 and abs(pointer['y'] - located['point'][1]) <= 1,
          'X server reports the core pointer at the grip centre', pointer)
    return rel


def begin_carry(s, display, located, case):
    check(s.overlays() == [], f'{case}: no carry overlay exists before the press', s.windows(False))
    check(s.x11.grab_state() == 'free', f'{case}: nothing holds the pointer before the press')
    arm_grip(s, display, located)
    mark = s.mark()
    started = time.monotonic()
    s.press()
    wait_for(lambda: s.x11.pointer()['button1'], 1, f'{case}: the X server to report the held physical button')
    check(True, f'{case}: X server reports the held physical button')
    overlay = wait_for(lambda: s.titled(OVERLAY_TITLE), 5, f'{case}: the native carry overlay to map after the grip press')
    check(len(overlay) == 1, f'{case}: exactly one carry overlay window', overlay)
    overlay = overlay[0]
    check(overlay != located['notch']['xid'], f'{case}: the overlay is a separate XID from the notch')
    work = located['notch']['record']['work']
    wait_for(lambda: same_rect(s.geometry(overlay), work, 1), 3, f'{case}: the overlay to cover the logged work area {work}')
    check(True, f'{case}: carry overlay occupies the work area', {'overlay': s.geometry(overlay), 'work': work})
    check(s.x11.grab_state() == 'held', f'{case}: the application holds the pointer grab while carrying')
    # dropzones.rs/carry_session.rs end the session ("Carry overlay did not paint") and destroy the
    # overlay if its page does not confirm a painted scene within 2 s. Outliving that deadline with no
    # session error is native evidence that the overlay painted; the capture is the visual record.
    remaining = 2.4 - (time.monotonic() - started)
    if remaining > 0:
        time.sleep(remaining)
    errors = [l for l in s.since(mark) if CARRY_ERROR.match(l)]
    check(not errors, f'{case}: no carry session error after the overlay paint deadline', errors)
    check(s.titled(OVERLAY_TITLE) == [overlay], f'{case}: the overlay is still mapped after the paint deadline')
    path, deviation = s.capture(overlay, f'x11-carry-{case}-overlay.png')
    check(deviation > 0.002, f'{case}: the captured overlay is not uniform (painted body/cells)', {'path': path, 'standard_deviation': deviation})
    return {'overlay': overlay, 'overlay_deviation': deviation, 'mark': mark}


def terminal_cleanup(s, case, overlay):
    wait_for(lambda: s.overlays() == [], 4, f'{case}: the carry overlay to be destroyed')
    check(True, f'{case}: carry overlay destroyed after the terminal', {'overlay': overlay})


def carry_release(s, display):
    before = s.config()
    check(before.get('notch_edge') == 'left', 'Release case starts from the seeded left edge', placement(before))
    located = locate_grip(s, display)
    begun = begin_carry(s, display, located, 'release')
    work = located['notch']['record']['work']
    wx, wy, ww, wh = work
    start = s.x11.pointer()
    step = 24
    path = []
    yy = start['y']
    while yy > wy + 3:
        path.append((wx + 3, yy))
        yy -= step
    path.append((wx + 3, wy + 3))
    xx = wx + 3
    while xx < wx + ww / 2:
        path.append((xx, wy + 3))
        xx += 2 * step
    path.append((wx + ww / 2, wy + 3))
    for index, (x, y) in enumerate(path):
        s.move(x, y, 0.03)
        if index == len(path) // 2:
            check(s.x11.pointer()['button1'] and s.titled(OVERLAY_TITLE) == [begun['overlay']],
                  'Button 1 stays held and the overlay stays mapped mid-traversal')
    time.sleep(0.4)
    check(s.x11.grab_state() == 'held', 'release: the grab is still held at the end of the traversal')
    s.capture_root('x11-carry-release-held.png')
    check(not [l for l in s.since(begun['mark']) if CARRIED.match(l) or CARRY_ERROR.match(l)],
          'No carry terminal before the physical release')
    s.release()
    wait_for(lambda: not s.x11.pointer()['button1'], 1, 'the X server to report the button released')
    check(True, 'X server reports the button released')

    def carried():
        for line in s.since(begun['mark']):
            if CARRY_ERROR.match(line):
                raise Failure(f'Native carry ended with an error after release: {line}')
            match = CARRIED.match(line)
            if match:
                return match.groups()
        return None
    edge, ratio, monitor = wait_for(carried, 12, 'the native "notch carried to" record after release')
    check(edge == 'top', 'Release commits the top edge reached by the physical traversal', edge)
    terminal_cleanup(s, 'release', begun['overlay'])
    check(s.x11.grab_state() == 'free', 'release: the pointer grab is released after the terminal')
    after = s.config()
    saved = placement(after)
    check(saved['edge'] == 'top', 'Saved placement edge is top', saved)
    check(abs(float((saved['along'] or {}).get('top', -1)) - float(ratio)) < 1e-5, 'Saved top position equals the logged landing ratio', {'saved': saved, 'logged': ratio})
    check(monitor_matches(monitor, saved['monitor']), 'Saved monitor equals the logged landing monitor', {'saved': saved, 'logged': monitor})
    check(saved != placement(before), 'Successful carry persists a changed placement', {'before': placement(before), 'after': saved})
    landed = wait_for(lambda: s.notch_matches_log(begun['mark'], 'top'), 5, 'the notch XID to match its logged top placement')
    check(abs(landed['rect'][1] - wy) <= 1, 'The actual notch window sits on the work-area top', landed)
    s.capture_root('x11-carry-release-landed.png')
    return {'case': 'physical-grip-release', 'grip': located['grip'], 'hot_rects': located['rects'],
            'overlay': begun['overlay'], 'overlay_deviation': begun['overlay_deviation'], 'path_points': len(path),
            'logged': {'edge': edge, 'ratio': ratio, 'monitor': monitor}, 'before': placement(before), 'after': saved,
            'notch': landed}


def carry_escape(s, display):
    before = placement(s.config())
    located = locate_grip(s, display)
    original = located['notch']['rect']
    begun = begin_carry(s, display, located, 'escape')
    wx, wy, ww, wh = located['notch']['record']['work']
    start = s.x11.pointer()
    for k in range(1, 9):
        s.move(start['x'] + (wx + ww / 2 - start['x']) * k / 8, start['y'] + (wy + wh - 4 - start['y']) * k / 8, 0.04)
    time.sleep(0.3)
    check(s.titled(OVERLAY_TITLE) == [begun['overlay']] and not [l for l in s.since(begun['mark']) if CARRY_ERROR.match(l) or CARRIED.match(l)],
          'Escape trigger starts from an active, unterminated carry')
    check(s.x11.grab_state() == 'held', 'escape: the application still holds the pointer grab before Escape')
    triggered = time.monotonic()
    s.escape_down()

    def cancelled():
        for line in s.since(begun['mark']):
            if CARRIED.match(line):
                raise Failure(f'Escape committed a placement instead of cancelling: {line}')
            match = CARRY_ERROR.match(line)
            if match:
                return match.groups()
        return None
    carry_id, reason = wait_for(cancelled, 5, 'the native carry session to cancel on the physical Escape key')
    observed_ms = int((time.monotonic() - triggered) * 1000)
    check('cancel' in reason.lower(), 'The native session ended as a cancellation', reason)
    held = s.x11.pointer()
    check(held['button1'] and held['escape'], 'Cancellation observed while the button and Escape are still physically held', held)
    terminal_cleanup(s, 'escape', begun['overlay'])
    check(s.x11.grab_state() == 'free', 'escape: the pointer grab is released while the button is still held')
    s.escape_up()
    s.release()
    time.sleep(0.5)
    check(not [l for l in s.since(begun['mark']) if CARRIED.match(l)], 'The later hardware release commits nothing')
    errors = [l for l in s.since(begun['mark']) if CARRY_ERROR.match(l)]
    check(len(errors) == 1, 'Exactly one terminal record for the cancelled carry', errors)
    after = placement(s.config())
    check(after == before, 'Escape leaves every saved placement field unchanged', {'before': before, 'after': after})
    restored = wait_for(lambda: (lambda r: r if same_rect(r, original, 1) else None)(s.geometry(s.notch())), 4,
                        'the notch XID to return to its original rectangle')
    check(True, 'Escape restores the original native notch rectangle', {'original': original, 'restored': restored})
    pointer = s.x11.pointer()
    check(not pointer['button1'] and not pointer['escape'] and s.x11.grab_state() == 'free',
          'escape: no held button, key or grab remains', pointer)
    return {'case': 'physical-escape-cancel', 'id': carry_id, 'reason': reason, 'terminal_observed_ms': observed_ms,
            'held_at_terminal': held, 'placement': before, 'rect': original, 'overlay': begun['overlay']}


def accept(pid, work):
    s = Session(pid, work)
    results = []
    error = None
    output = os.path.join(work, 'native-carry.txt')
    try:
        display = [int(v) for v in run(['xdotool', 'getdisplaygeometry']).stdout.decode().split()]
        saved_pointer = s.x11.pointer()
        check(not saved_pointer['button1'] and not saved_pointer['escape'], 'No physical input is held at start', saved_pointer)
        results.append(carry_release(s, display))
        results.append(carry_escape(s, display))
        check(len(results) == 2, 'Both physical carry cases completed')
    except BaseException as caught:
        error = caught
        raise
    finally:
        cleanup = s.release_all()
        try:
            s.capture_root('x11-carry-final.png')
        except Exception as caught:
            cleanup.append(f'final capture: {caught}')
        record = {'passed': error is None and not cleanup, 'error': None if error is None else repr(error),
                  'cleanup_errors': cleanup, 'pid': pid, 'results': results}
        try:
            with open(output if record['passed'] else output.replace('.txt', '-partial.txt'), 'w', encoding='utf-8') as handle:
                json.dump(record, handle, indent=2, default=str)
        except Exception as caught:
            cleanup.append(f'receipt: {caught}')
        for message in cleanup:
            print(f'Carry cleanup: {message}', file=sys.stderr)
        # The primary error, if any, propagates unchanged; without one, failed cleanup is a failure.
        if error is None and cleanup:
            raise Failure('Physical carry cleanup failed: ' + '; '.join(cleanup))
    print('PASS: actual X11 grip press, perimeter traversal and release persisted to the top edge; physical Escape '
          'cancelled while held and restored the placement; overlay XID lifecycle, paint and pointer-grab release.')


def relaunched(pid, work, mark):
    """A fresh process places the notch from the carried, saved configuration."""
    s = Session(pid, work)
    saved = placement(s.config())
    check(saved['edge'] == 'top', 'Relaunch reads the carried top edge', saved)
    landed = wait_for(lambda: s.notch_matches_log(mark, 'top'), 15, 'the relaunched notch XID to match its logged top placement')
    check(abs(landed['rect'][1] - landed['record']['work'][1]) <= 1, 'Relaunched notch window sits on the work-area top', landed)
    with open(os.path.join(work, 'native-carry-relaunch.txt'), 'w', encoding='utf-8') as handle:
        json.dump({'passed': True, 'pid': pid, 'placement': saved, 'notch': landed}, handle, indent=2)


def main(argv):
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_OS') != 'Linux' or not os.environ.get('DISPLAY'):
        print('Physical X11 carry runs only on the disposable Linux CI display', file=sys.stderr)
        return 2
    # The harness timeout sends SIGTERM first: turn it into an exception so `finally` releases input.
    def terminated(signum, frame):
        raise Failure(f'Terminated by signal {signum} (harness timeout)')
    signal.signal(signal.SIGTERM, terminated)
    mode, pid, work = argv[1], int(argv[2]), argv[3]
    if pid <= 1 or pid == os.getpid():
        raise Failure(f'Invalid application PID {pid}')
    if mode == 'carry':
        accept(pid, work)
    elif mode == 'relaunch':
        relaunched(pid, work, int(argv[4]))
    else:
        raise Failure(f'Unknown mode {mode}')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
