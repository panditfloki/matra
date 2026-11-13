'use strict';
// Physical X11 carry acceptance (scripts/linux-native-carry.py). The real run needs the disposable
// Linux CI display and the packaged app; these checks cover what runs anywhere: Python syntax, the
// driver's pure parsing/grip identification on the exact native log formats, the coupling of its
// patterns to the native source strings, and the shell wiring/input release. No display is touched.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const driver = path.join(root, 'scripts/linux-native-carry.py');
const harness = path.join(root, 'scripts/test-linux-development.sh');
const python = ['python3'].find(p => spawnSync(p, ['--version']).status === 0);
const read = rel => fs.readFileSync(path.join(root, rel), 'utf8');

function py(body, env = {}) {
  // Loads the driver as a module without running main(); -I keeps the scratch-free import isolated.
  const prelude = `import importlib.util, json, sys
spec = importlib.util.spec_from_file_location('carry', sys.argv[1])
carry = importlib.util.module_from_spec(spec); spec.loader.exec_module(carry)
`;
  return spawnSync(python, ['-I', '-B', '-c', prelude + body, driver], { encoding: 'utf8', timeout: 20000, env: { ...process.env, ...env } });
}

test('carry driver is valid Python and refuses to run outside the disposable Linux CI display', { skip: !python && 'python3 unavailable' }, () => {
  const compile = spawnSync(python, ['-I', '-B', '-c', 'import sys; compile(open(sys.argv[1]).read(), sys.argv[1], "exec")', driver]);
  assert.equal(compile.status, 0, String(compile.stderr));
  const env = { ...process.env, GITHUB_ACTIONS: '', RUNNER_OS: '', DISPLAY: '' };
  const refused = spawnSync(python, ['-I', '-B', driver, 'carry', '4242', '/nonexistent'], { encoding: 'utf8', env });
  assert.equal(refused.status, 2);
  assert.match(refused.stderr, /only on the disposable Linux CI display/);
});

test('driver patterns match the native log lines and window titles they rely on', { skip: !python && 'python3 unavailable' }, () => {
  const main = read('native/matra/src/main.rs');
  const session = read('native/matra/src/carry_session.rs');
  // The exact Rust format strings the driver parses. A change there must fail here, not in CI.
  assert.ok(main.includes('"click-through {} at cursor_rel=({lx:.0},{ly:.0}) rects={rects:?}"'));
  assert.ok(main.includes('if inside { "off (cursor on the notch)" } else { "on (cursor elsewhere)" }'));
  assert.ok(main.includes('"notch placed build={BUILD}: edge={edge} pos=({x},{y}) size=({ww}x{wh}) inner={:?} win_scale={scale} mon_scale={ms} notch_size={size} monitor={:?}=({},{} {}x{}) work={:?} card_insets_css={insets:?}"'));
  assert.ok(session.includes('"notch carried to {} at {ratio:.6} on {:?}"'));
  assert.ok(session.includes('crate::applog(&format!("notch carry {id}: {error}"));'));
  assert.ok(read('native/matra/src/dropzones.rs').includes('.title("Matra carry")'));
  assert.equal(JSON.parse(read('native/matra/tauri.conf.json')).app.windows.find(w => w.label === 'notch').title, 'dy/dx.f(Mātrā)');
  // The rendered forms of those strings, as Rust prints them.
  const lines = {
    off: 'click-through off (cursor on the notch) at cursor_rel=(14,301) rects=[[0.0, 120.5, 30.0, 180.0], [2.5, 330.0, 25.0, 25.0], [2.5, 300.5, 25.0, 29.5], [2.5, 300.0, 25.0, 25.0], [2.5, 300.5, 25.0, 0.0]]',
    on: 'click-through on (cursor elsewhere) at cursor_rel=(1500,900) rects=[]',
    placed: 'notch placed build=0.2.0-dev (abc): edge=top pos=(752,0) size=(416x416) inner=(416, 416) win_scale=1 mon_scale=1 notch_size=0.8 monitor=Some("screen")=(0,0 1920x1080) work=(0, 0, 1920, 1080) card_insets_css=[0.0, 0.0, 0.0, 0.0]',
    carried: 'notch carried to top at 0.500000 on Some("screen")',
    error: 'notch carry 1763000000123: Carry cancelled or capture lost',
  };
  const r = py(`lines = json.loads(sys.argv[2])
off = carry.CLICK_OFF.match(lines['off'])
print(json.dumps({
  'off': [off.group(1), off.group(2), carry.parse_rects(off.group(3))],
  'on': bool(carry.CLICK_ON.match(lines['on'])), 'on_is_off': bool(carry.CLICK_OFF.match(lines['on'])),
  'placed': carry.PLACED.search(lines['placed']).groups(),
  'carried': carry.CARRIED.match(lines['carried']).groups(),
  'error': carry.CARRY_ERROR.match(lines['error']).groups(),
  'monitor': [carry.monitor_matches('Some("screen")', 'screen'), carry.monitor_matches('None', None),
              carry.monitor_matches('Some("screen")', None), carry.monitor_matches('None', 'screen')],
  'title': carry.NOTCH_TITLE}))`.replace('sys.argv[2]', JSON.stringify(JSON.stringify(lines))));
  assert.equal(r.status, 0, r.stderr);
  const out = JSON.parse(r.stdout);
  assert.deepEqual(out.off.slice(0, 2), ['14', '301']);
  assert.equal(out.off[2].length, 5);
  assert.equal(out.on, true);
  assert.equal(out.on_is_off, false);
  assert.deepEqual(out.placed, ['top', '752', '0', '416', '416', '0', '0', '1920', '1080']);
  assert.deepEqual(out.carried, ['top', '0.500000', 'Some("screen")']);
  assert.deepEqual(out.error, ['1763000000123', 'Carry cancelled or capture lost']);
  assert.deepEqual(out.monitor, [true, true, false, false]);
  assert.equal(out.title, 'dy/dx.f(Mātrā)');
});

test('grip is the first circle+bridge pair beyond the settings orb, on both edge axes', { skip: !python && 'python3 unavailable' }, () => {
  // notch.html reportHot(): pill (and card/tail/alert surfaces) first, then addHandle(move), addHandle(orb):
  // each a square circle followed by a bridge as wide as the circle across the edge axis.
  const left = [[0, 120, 30, 180], [2.5, 330, 25, 25], [2.5, 300.5, 25, 29.5], [2.5, 300, 25, 25], [2.5, 300, 25, 0]];
  const leftWithCard = [[0, 120, 30, 180], [30, 140, 300, 160], [28, 150, 4, 20], ...left.slice(1)];
  const top = [[100, 0, 200, 30], [330, 2, 26, 26], [300, 2, 30, 26], [300, 2, 26, 26], [300, 2, 0, 26]];
  const r = py(`cases = json.loads(sys.argv[2])
out = {}
for name, edge, rects in cases:
    try: out[name] = carry.grip_rect(rects, edge)
    except carry.Failure as e: out[name] = 'FAIL: ' + str(e)
print(json.dumps(out))`.replace('sys.argv[2]', JSON.stringify(JSON.stringify([
    ['left', 'left', left], ['leftWithCard', 'left', leftWithCard], ['top', 'top', top],
    ['noGrip', 'left', left.slice(0, 3)], ['pillOnly', 'top', [top[0]]],
    // A grip that is not beyond the orb is not accepted as the grip.
    ['swapped', 'left', [left[0], left[3], left[4], left[1], left[2]]],
    // Axis matters: a horizontal-edge bridge on a vertical edge is not a bridge.
    ['wrongAxis', 'left', top],
  ]))));
  assert.equal(r.status, 0, r.stderr);
  const out = JSON.parse(r.stdout);
  assert.deepEqual(out.left, [2.5, 330, 25, 25]);
  assert.deepEqual(out.leftWithCard, [2.5, 330, 25, 25]);
  assert.deepEqual(out.top, [330, 2, 26, 26]);
  assert.match(out.noGrip, /^FAIL: Expected the grip and settings handles.*found 1/);
  assert.match(out.pillOnly, /found 0/);
  assert.match(out.swapped, /not beyond the settings orb/);
  assert.match(out.wrongAxis, /^FAIL/);
});

test('driver releases held input in finally without replacing the original error', { skip: !python && 'python3 unavailable' }, () => {
  // Session.release_all with xdotool shadowed by a recorder: only held inputs are released, every
  // release is attempted even if one fails, and accept() re-raises the primary failure.
  const scratch = path.join(root, '.build', 'test-linux-native-carry');
  fs.rmSync(scratch, { recursive: true, force: true });
  fs.mkdirSync(path.join(scratch, 'bin'), { recursive: true });
  fs.writeFileSync(path.join(scratch, 'bin', 'xdotool'), `#!/bin/sh\necho "$*" >> "${scratch}/xdotool.txt"\n[ "$1" != mouseup ] || [ -z "$FAIL_MOUSEUP" ] || exit 1\n`, { mode: 0o755 });
  try {
    const r = py(`s = carry.Session.__new__(carry.Session)
s.held_button, s.held_escape = True, True
print(json.dumps(s.release_all()))
s.held_button, s.held_escape = False, False
print(json.dumps(s.release_all()))`, { PATH: `${path.join(scratch, 'bin')}${path.delimiter}${process.env.PATH}`, FAIL_MOUSEUP: '1' });
    assert.equal(r.status, 0, r.stderr);
    const [first, second] = r.stdout.trim().split('\n').map(l => JSON.parse(l));
    assert.equal(first.length, 1);
    assert.match(first[0], /^left button: xdotool mouseup failed/);
    assert.deepEqual(second, []);
    assert.deepEqual(fs.readFileSync(path.join(scratch, 'xdotool.txt'), 'utf8').trim().split('\n'), ['mouseup 1', 'keyup Escape']);
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
  const source = fs.readFileSync(driver, 'utf8');
  assert.match(source, /except BaseException as caught:\n {8}error = caught\n {8}raise\n {4}finally:\n {8}cleanup = s\.release_all\(\)/);
  assert.match(source, /if error is None and cleanup:\n {12}raise Failure/);
  assert.match(source, /signal\.signal\(signal\.SIGTERM, terminated\)/);
  // Real input only: no page evaluation, IPC, CDP or synthetic DOM events anywhere in the driver.
  assert.doesNotMatch(source, /invoke\(|__TAURI__|evaluate|dispatchEvent|native-inspect|devtools|websocket/i);
});

function shellFunction(text, name) {
  const match = text.match(new RegExp(`^${name}\\(\\)\\{.*\\}$|^${name}\\(\\)\\{\\n[\\s\\S]*?^\\}$`, 'm'));
  assert.ok(match, `${name} not found`);
  return match[0];
}

test('harness runs the carry phase after the unchanged gates and releases input from the EXIT trap', () => {
  assert.equal(spawnSync('bash', ['-n', harness]).status, 0);
  const text = fs.readFileSync(harness, 'utf8');
  // Order: settings gate, relaunch, AppImage, then the carry phase on a freshly painted process.
  const at = s => { const i = text.indexOf(s); assert.ok(i >= 0, s); return i; };
  assert.ok(at('phase=settings\n') < at('phase=relaunch\n'));
  assert.ok(at('phase=relaunch\n') < at('phase=appimage\n'));
  assert.ok(at('phase=appimage\n') < at('phase=carry\n'));
  assert.ok(at('phase=carry\n') < at('phase=carry-relaunch\n'));
  assert.ok(at('phase=carry-relaunch\n') < at('> "$work/RESULT.txt"'));
  assert.match(text, /phase=carry\n"\$exe" --silent [^\n]*& app=\$!\nwait_settings\ncheck_app\ncarry_input=1\ntimeout --kill-after=5 240 python3 -I -B "\$repo\/scripts\/linux-native-carry\.py" carry "\$app" "\$work"\n/);
  assert.match(text, /phase=carry-relaunch\n[^\n]*\n"\$exe" --silent [^\n]*& app=\$!\nwait_settings\ntimeout --kill-after=5 60 python3 -I -B "\$repo\/scripts\/linux-native-carry\.py" relaunch /);
  // The trap releases held input first, before diagnostics or stopping the app, and keeps its status.
  const cleanup = shellFunction(text, 'cleanup');
  assert.match(cleanup, /local status=\$\?\n {2}set \+e\n {2}release_carry_input\n/);
  assert.match(cleanup, /return "\$status"\n\}$/);
  const release = shellFunction(text, 'release_carry_input');
  const run = flag => spawnSync('bash', ['-c', `${release}
exec 3>&1
timeout(){ shift; "$@"; }
# fd 3 survives the function's own >/dev/null 2>&1; a failing release must not fail the trap.
xdotool(){ echo "XDOTOOL $*" >&3; return 1; }
carry_input='${flag}'
release_carry_input; echo "rc=$? flag=[$carry_input]"`], { encoding: 'utf8' });
  const held = run('1');
  assert.equal(held.status, 0, held.stderr);
  assert.deepEqual(held.stdout.trim().split('\n'), ['XDOTOOL mouseup 1', 'XDOTOOL keyup Escape', 'rc=0 flag=[]']);
  const idle = run('');
  assert.deepEqual(idle.stdout.trim().split('\n'), ['rc=0 flag=[]']);
});
