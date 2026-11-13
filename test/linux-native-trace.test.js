'use strict';
// Diagnostic-only Linux trace helpers. The traced CI run itself needs Linux, X11, gdb and
// strace; these checks cover what can run anywhere: shell syntax, the gdb script's Python
// syntax and the strace summariser on a synthetic capture. Scratch files stay in .build/.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const python = ['python3'].find(p => spawnSync(p, ['--version']).status === 0);

test('Linux lifecycle harness parses and keeps the real gate before diagnostics', () => {
  const script = path.join(root, 'scripts/test-linux-development.sh');
  assert.equal(spawnSync('bash', ['-n', script]).status, 0);
  const text = fs.readFileSync(script, 'utf8');
  // The paint gate and its threshold are unchanged; diagnostics run only from cleanup on failure.
  assert.match(text, /float\(sys\.argv\[1\]\) > 0\.01/);
  assert.match(text, /if \(\( status != 0 \)\) && \[\[ -n "\$phase" && "\$\{MATRA_NATIVE_TRACE:-1\}" != 0 \]\]; then\n    native_trace/);
  assert.match(text, /return "\$status"\n\}/);
  // Window ownership is checked against the real app PID read from the tracer, never the tracer PID,
  // and only after the whole PID line has been validated (no digit filtering of '-5' into '5').
  assert.match(text, /IFS= read -r candidate < "\$trace\/gdb-app\.pid"/);
  assert.match(text, /if valid_pid "\$candidate"; then\n      app="\$candidate" phase=diagnostic-gdb/);
  assert.match(text, /if valid_pid "\$candidate"; then\n      app="\$candidate" phase=diagnostic-strace/);
  assert.doesNotMatch(text, /tr -dc 0-9/);
  assert.match(text, /check_app \|\| return 1/);
  // Inside the diagnostics every direct signal to $app or $tracer is guarded on the same line;
  // stop_app and check_app guard their own entry (exercised in the next test).
  const diagnostics = shellFunction(text, 'native_trace');
  const signals = diagnostics.split('\n').filter(l => /kill -[A-Z]+ "\$(app|tracer)"/.test(l));
  assert.equal(signals.length, 4);
  for (const line of signals) assert.match(line, /valid_pid "\$(app|tracer)" && kill -0/, line);
});

function shellFunction(text, name) {
  // One-line definitions first, otherwise the lazy multi-line form runs on to the next function.
  const match = text.match(new RegExp(`^${name}\\(\\)\\{.*\\}$|^${name}\\(\\)\\{\\n[\\s\\S]*?^\\}$`, 'm'));
  assert.ok(match, `${name} not found`);
  return match[0];
}

test('invalid application PIDs never reach kill or window polling', () => {
  const text = fs.readFileSync(path.join(root, 'scripts/test-linux-development.sh'), 'utf8');
  const functions = ['valid_pid', 'stop_app', 'check_app'].map(n => shellFunction(text, n)).join('\n');
  // kill and wait are shadowed by functions, so nothing is ever really signalled here.
  const script = `${functions}
kill(){ printf 'KILL %s\\n' "$*"; return 1; }
wait(){ printf 'WAIT %s\\n' "$*"; }
work=/nonexistent tracer=x phase=t
for v in '' 0 1 -1 -5 00 012 12abc ' 12' '12 ' 0x10 '1 2' 99999999999 4242; do
  if valid_pid "$v"; then echo "valid[$v]"; else echo "invalid[$v]"; fi
done
if valid_pid "$$"; then echo self-valid; else echo self-invalid; fi
for v in 0 -1 1 '' 12abc "$$"; do app="$v"; echo "stop[$v]"; stop_app; echo "after[$app]"; echo "check[$v]"; check_app 2>/dev/null; echo "rc=$?"; done
app=4242; echo stop[4242]; stop_app >/dev/null; echo "after[$app]"
`;
  const run = spawnSync('bash', ['-c', script], { encoding: 'utf8', timeout: 20000 });
  assert.equal(run.status, 0, run.stderr);
  const out = run.stdout;
  for (const v of ['', '0', '1', '-1', '-5', '00', '012', '12abc', ' 12', '12 ', '0x10', '1 2', '99999999999']) {
    assert.ok(out.includes(`invalid[${v}]`), `accepted ${JSON.stringify(v)}`);
  }
  assert.deepEqual(out.match(/^valid\[.*\]$/gm), ['valid[4242]']);
  assert.match(out, /^self-invalid$/m, "this shell's own PID is rejected");
  // Invalid values: no kill at all, app cleared by stop_app, check_app reports failure.
  const [invalidPart, validPart] = out.split('stop[4242]');
  assert.doesNotMatch(invalidPart, /^KILL /m, 'an invalid PID reached kill');
  assert.equal((invalidPart.match(/^rc=1$/gm) || []).length, 6);
  assert.equal((invalidPart.match(/^after\[\]$/gm) || []).length, 6);
  // A real child PID keeps the original stop semantics (the shadowed kill only prints).
  assert.match(validPart, /after\[\]/);
});

test('harness functions state every return status (bare return misreports inside the EXIT trap)', () => {
  // bash 5.1 (ubuntu-22.04): a bare `return` in a function run from a trap handler returns the status
  // saved before the trap, i.e. the original failure. CI 43453950415 lost its strace reproduction
  // that way: check_app said the live app had ended, silently, and the app was signalled in ld.so.
  const text = fs.readFileSync(path.join(root, 'scripts/test-linux-development.sh'), 'utf8');
  const bare = text.split('\n').map((l, i) => [i + 1, l.replace(/#.*$/, '')])
    .filter(([, l]) => /(^|[;{]|\bthen|\belse|\bdo|&&|\|\|)\s*return\s*($|;|\}|\bfi\b)/.test(l));
  assert.deepEqual(bare, [], 'bare return');
});

test('the diagnostic wait inside the EXIT trap keeps polling a live app and keeps the paint gate', { skip: !python && 'python3 unavailable' }, () => {
  const text = fs.readFileSync(path.join(root, 'scripts/test-linux-development.sh'), 'utf8');
  const functions = ['valid_pid', 'check_app', 'wait_settings'].map(n => shellFunction(text, n)).join('\n');
  const scratch = path.join(root, '.build', 'test-linux-native-trace-harness');
  fs.rmSync(scratch, { recursive: true, force: true });
  const bin = path.join(scratch, 'bin');
  fs.mkdirSync(bin, { recursive: true });
  // Stand-ins for the X11/ImageMagick tools. They never touch a display.
  const stub = (name, body) => fs.writeFileSync(path.join(bin, name), `#!/bin/sh\n${body}\n`, { mode: 0o755 });
  stub('xdotool', `case "$1" in
  search) echo x >> "$STUB_DIR/searches"; [ -z "$STUB_WINDOW" ] || echo "$STUB_WINDOW";;
  getwindowpid) echo "$STUB_APP_PID";;
  getwindowgeometry) echo "WINDOW=$3";;
esac`);
  stub('import', 'for a; do last="$a"; done; : > "$last"');
  stub('convert', 'for a; do last="$a"; done; : > "$last"');
  stub('identify', 'case "$2" in "%w %h"*) echo "400 600";; *) echo "$STUB_DEVIATION";; esac');
  const run = (scenario, env) => {
    const dir = path.join(scratch, scenario);
    fs.mkdirSync(path.join(dir, 'work', 'screenshots'), { recursive: true });
    fs.mkdirSync(path.join(dir, 'work', 'native-trace'), { recursive: true });
    // The real functions run from an EXIT trap after a failing command, exactly as native_trace
    // calls them; `sleep` is shadowed so 120 polls take no time, the app is a real live child.
    const script = `set -euo pipefail
${functions}
sleep(){ :; }
work="$STUB_DIR/work" app='' tracer='' phase=''
cleanup(){
  local status=$?
  set +e
  if [ "$SCENARIO" = dead ]; then /bin/sleep 0 & app=$!; command wait "$app"; else /bin/sleep 30 & app=$!; fi
  export STUB_APP_PID="$app"
  tracer=diagnostic phase=diagnostic-test
  if wait_settings; then echo WAIT=0; else echo WAIT=1; fi
  [ "$SCENARIO" = dead ] || { kill "$app"; command wait "$app"; }
  return "$status"
}
trap cleanup EXIT
false
`;
    const r = spawnSync('bash', ['-c', script], { encoding: 'utf8', timeout: 60000,
      env: { ...process.env, ...env, SCENARIO: scenario, STUB_DIR: dir, PATH: `${bin}${path.delimiter}${process.env.PATH}` } });
    const read = name => fs.existsSync(path.join(dir, name)) ? fs.readFileSync(path.join(dir, name), 'utf8') : '';
    return { ...r, searches: read('searches').split('\n').filter(Boolean).length, paint: read('work/diagnostic-test-paint.txt'), ended: read('work/native-trace/process-exit.txt') };
  };
  try {
    const blank = run('blank', { STUB_WINDOW: '', STUB_DEVIATION: '0.5' });
    assert.equal(blank.status, 1, 'the trap still returns the original failure');
    assert.match(blank.stdout, /^WAIT=1$/m);
    assert.equal(blank.searches, 120, 'a live app is polled for the whole window, not dropped at once');
    assert.match(blank.stderr, /did not remain visible and paint \(diagnostic-test\)/);
    assert.equal(blank.ended, '');

    const painted = run('painted', { STUB_WINDOW: '77', STUB_DEVIATION: '0.5' });
    assert.match(painted.stdout, /^WAIT=0$/m, 'a painted traced app is reported as such');
    assert.equal(painted.searches, 5, 'four stable repeats of the same owned window before capture');
    assert.match(painted.paint, /XID=77 .*content_standard_deviation=0\.5/);

    const uniform = run('uniform', { STUB_WINDOW: '77', STUB_DEVIATION: '0.01' });
    assert.match(uniform.stdout, /^WAIT=1$/m, 'uniform content never passes the >0.01 gate');
    assert.equal(uniform.searches, 120);
    assert.equal(uniform.paint, '');

    const dead = run('dead', { STUB_WINDOW: '77', STUB_DEVIATION: '0.5' });
    assert.match(dead.stdout, /^WAIT=1$/m);
    assert.equal(dead.searches, 0, 'an ended app is not polled');
    assert.match(dead.ended, /ended phase=diagnostic-test/);
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
});

function runGdbScript(mode) {
  const scratch = path.join(root, '.build', 'test-linux-native-trace-gdb', mode);
  fs.rmSync(scratch, { recursive: true, force: true });
  const stub = path.join(scratch, 'stub');
  const out = path.join(scratch, 'out');
  fs.mkdirSync(stub, { recursive: true });
  fs.mkdirSync(out, { recursive: true });
  // Minimal stand-in for gdb's Python API. It never starts or signals a process.
  fs.writeFileSync(path.join(stub, 'gdb.py'), `
import os
MODE = os.environ['STUB_MODE']
class error(RuntimeError): pass
class MemoryError(error): pass
class BreakpointEvent: pass
class SignalEvent: pass
class _Inferior:
    pid = 0
    def is_valid(self): return True
_inferior = _Inferior()
commands = []
def execute(command, to_string=False):
    commands.append(command)
    with open(os.path.join(os.environ['MATRA_TRACE_DIR'], 'stub-commands.txt'), 'a') as h:
        h.write(command + '\\n')
    if command == 'starti':
        if MODE == 'fail': raise error('During startup program exited with code 127.')
        if MODE == 'valid': _inferior.pid = 4242
        return ''
    if command == 'continue':
        _inferior.pid = 0
        return ''
    if command.startswith('quit'):
        raise SystemExit(int(command.split()[1]) if ' ' in command else 0)
    return ''
def selected_inferior(): return _inferior
def selected_thread(): return None
def breakpoints(): return ()
class Breakpoint:
    def __init__(self, spec): self.location = spec
class _Registry:
    def connect(self, fn): pass
class events:
    stop = _Registry()
    exited = _Registry()
`);
  const source = path.join(root, 'scripts/linux-native-trace-gdb.py');
  const run = spawnSync(python, ['-I', '-B', '-c',
    'import sys; sys.path.insert(0, sys.argv[1]); exec(compile(open(sys.argv[2]).read(), sys.argv[2], "exec"), {"__name__": "__main__"})',
    stub, source], { env: { ...process.env, MATRA_TRACE_DIR: out, STUB_MODE: mode }, encoding: 'utf8', timeout: 20000 });
  const read = name => fs.existsSync(path.join(out, name)) ? fs.readFileSync(path.join(out, name), 'utf8') : null;
  const result = { status: run.status, stderr: run.stderr, pidFile: read('gdb-app.pid'), exit: read('gdb-exit.json'), commands: read('stub-commands.txt') || '' };
  fs.rmSync(scratch, { recursive: true, force: true });
  return result;
}

test('gdb script rejects a failed start and a zero PID explicitly', { skip: !python && 'python3 unavailable' }, () => {
  for (const mode of ['fail', 'zero']) {
    const r = runGdbScript(mode);
    assert.equal(r.status, 3, `${mode}: ${r.stderr}`);
    assert.equal(r.pidFile, null, `${mode}: no application PID may be published`);
    const exit = JSON.parse(r.exit);
    assert.equal(exit.end, 'start failed');
    assert.equal(exit.pid, null);
    assert.match(exit.reason, mode === 'fail' ? /starti failed: During startup/ : /invalid application pid 0/);
    assert.doesNotMatch(r.commands, /^(continue|kill)$/m, `${mode}: nothing runs after a failed start`);
  }
});

test('gdb script still publishes the real child PID after a good start', { skip: !python && 'python3 unavailable' }, () => {
  const r = runGdbScript('valid');
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.pidFile, '4242\n');
  const exit = JSON.parse(r.exit);
  assert.equal(exit.pid, 4242);
  assert.equal(exit.end, 'exited');
});

test('gdb trace script is valid Python', { skip: !python && 'python3 unavailable' }, () => {
  const source = path.join(root, 'scripts/linux-native-trace-gdb.py');
  const check = spawnSync(python, ['-I', '-c', 'import sys; compile(open(sys.argv[1]).read(), sys.argv[1], "exec")', source]);
  assert.equal(check.status, 0, String(check.stderr));
});

test('strace summary finds the ending thread, X11 operations and compresses the capture', { skip: !python && 'python3 unavailable' }, () => {
  const scratch = path.join(root, '.build', 'test-linux-native-trace');
  fs.rmSync(scratch, { recursive: true, force: true });
  fs.mkdirSync(scratch, { recursive: true });
  const capture = path.join(scratch, 'strace.txt');
  const x11 = '<UNIX-STREAM:[4242->4243,@/tmp/.X11-unix/X99]>';
  fs.writeFileSync(capture, [
    '100   15:09:22.100000 execve("/usr/bin/matra-development020", ["matra-development020", "--silent"], 0x7ffd /* 30 vars */) = 0 <0.000300>',
    `100   15:09:22.200000 writev(3${x11}, [{iov_base="\\x01\\x02", iov_len=2}], 1) = 2 <0.000010>`,
    '100   15:09:22.300000 clone3({flags=CLONE_VM|CLONE_FS|CLONE_FILES|CLONE_SIGHAND|CLONE_THREAD|CLONE_SYSVSEM, stack=0x7f, stack_size=0x1000} <unfinished ...>',
    '101   15:09:22.300100 set_robust_list(0x7f, 24) = 0 <0.000004>',
    '100   15:09:22.300200 <... clone3 resumed> => {parent_tid=[101]}, 88) = 101 <0.000050>',
    '100   15:09:22.400000 clone(child_stack=NULL, flags=CLONE_CHILD_CLEARTID|CLONE_CHILD_SETTID|SIGCHLD, child_tidptr=0x7f) = 200 <0.000100>',
    '200   15:09:22.410000 execve("/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess", ["7", "45"], 0x7f /* 30 vars */) = 0 <0.000300>',
    `101   15:09:22.900000 recvmsg(3${x11}, {msg_namelen=0}, 0) = 0 <0.000020>`,
    '101   15:09:22.900100 exit_group(1)            = ?',
    '200   15:09:22.900200 read(5, "", 8) = 0 <0.000003>',
    '101   15:09:22.900300 +++ exited with 1 +++',
    '100   15:09:22.900400 +++ exited with 1 +++',
    '',
  ].join('\n'));
  const summary = path.join(scratch, 'summary.txt');
  const run = spawnSync(python, ['-I', path.join(root, 'scripts/linux-native-trace-summary.py'), capture, summary]);
  assert.equal(run.status, 0, String(run.stderr));
  const text = fs.readFileSync(summary, 'utf8');
  const head = JSON.parse(text.split('\n')[1]);
  assert.equal(head.main_pid, 100);
  assert.equal(head.exit_status, 'exited with 1');
  assert.deepEqual(head.exit_group_threads, [101]);
  assert.deepEqual(head.other_processes, [200], 'the forked WebKit process is not an application thread');
  assert.equal(head.x11_count, 2);
  assert.match(text, /## Last X11 socket operations[\s\S]*101 15:09:22\.900000 recvmsg/);
  assert.doesNotMatch(text.split('## Last syscalls across')[1].split('## Last syscalls per')[0], /^200 /m);
  assert.ok(fs.existsSync(capture + '.gz'));
  assert.ok(!fs.existsSync(capture), 'raw capture is replaced by the compressed one');
  fs.rmSync(scratch, { recursive: true, force: true });
});
