"""DIAGNOSTIC ONLY. gdb script loaded by scripts/test-linux-development.sh in disposable CI.

It runs after an application phase has already failed, and it cannot turn that failure into a
pass. It launches the same Linux development binary once more in an isolated fixture profile
and records native stacks where a process can end: libc exit()/_exit()/abort() and friends,
Xlib's fatal connection path (_XIOError) and fatal signals. A few cheap X11/GTK markers
(_XError, XCloseDisplay, xcb_disconnect, gtk_main_quit, g_application_quit) get a short,
capped stack so the last X11/GTK operations before the end are visible.

Breakpoints only observe: no return value or argument is changed and no call is skipped.
No syscall catchpoint is used by default, so the run stays close to normal timing. A raw
exit_group that bypasses libc is still covered by the separate strace reproduction.

The harness stops a still-running app with SIGSTOP (delivered through ptrace), which ends the
run with one all-thread dump. The output files stay inside MATRA_TRACE_DIR.
"""
import datetime
import json
import os

import gdb

OUT = os.environ['MATRA_TRACE_DIR']
# Full dump: every thread's stack. These are the ways a process ends.
FULL = ('exit', '_exit', 'quick_exit', 'abort', '__assert_fail', '__libc_fatal', '_XIOError')
# Short stack, capped per marker, then the breakpoint is disabled.
LIGHT = {'_XError': 12, 'XCloseDisplay': 6, 'xcb_disconnect': 6, 'gtk_main_quit': 6, 'g_application_quit': 6}
MAX_FULL = 8
QUIET = ('SIGPIPE', 'SIGUSR1', 'SIGUSR2', 'SIGALRM', 'SIGCHLD', 'SIGWINCH', 'SIGPROF', 'SIGCONT',
         'SIGPWR', 'SIGXCPU', 'SIGVTALRM', 'SIGURG', 'SIGIO')
# An abort or fault must still be seen; SIGSTOP is the harness's request for a live dump.
LOUD = ('SIGSEGV', 'SIGBUS', 'SIGILL', 'SIGFPE', 'SIGABRT', 'SIGTERM', 'SIGHUP', 'SIGQUIT', 'SIGSTOP')

state = {'event': None, 'full': 0, 'light': {}, 'maps': False, 'exit': None}
log = open(os.path.join(OUT, 'gdb-stops.txt'), 'a', buffering=1)


def stamp():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec='milliseconds')


def write(text):
    log.write(text if text.endswith('\n') else text + '\n')


def run(command):
    try:
        return gdb.execute(command, to_string=True) or ''
    except gdb.error as error:  # gdb.MemoryError is a subclass
        return '<%s failed: %s>\n' % (command, error)


def write_atomic(name, text):
    path = os.path.join(OUT, name)
    with open(path + '.tmp', 'w') as handle:
        handle.write(text)
    os.replace(path + '.tmp', path)


def on_stop(event):
    state['event'] = event


def on_exit(event):
    code = getattr(event, 'exit_code', None)
    state['exit'] = code
    write(json.dumps({'event': 'exited', 'at': stamp(), 'exit_code': code}))


def errno_and_display(at_xio):
    # Inferior calls need explicit types: the release binary and system libraries carry no
    # debug info. Each call is independent and a failure is recorded, never fatal.
    write(run('p *((int*(*)(void))__errno_location)()'))
    if at_xio:
        write('Display passed to _XIOError and the GDK default display, for comparison:')
        write(run('p/x $rdi'))
        write(run('p ((char*(*)(void*))XDisplayString)((void*)$rdi)'))
        write(run('p/x ((void*(*)(void*))gdk_x11_display_get_xdisplay)(((void*(*)(void))gdk_display_get_default)())'))


def full_dump(reason):
    state['full'] += 1
    thread = gdb.selected_thread()
    write('\n=== %s FULL STOP #%d: %s; thread %s (LWP %s)' % (
        stamp(), state['full'], reason,
        thread.num if thread else '?', thread.ptid[1] if thread else '?'))
    write(run('info threads'))
    write('--- stopping thread')
    write(run('bt'))
    write(run('info registers rip rsp rdi rsi rdx'))
    write(run('x/6i $pc'))
    errno_and_display('_XIOError' in reason)
    if reason.startswith('signal'):
        write(run('p $_siginfo'))
    if not state['maps']:
        # Raw addresses plus mappings and build IDs allow offline symbolisation later.
        state['maps'] = True
        write('--- shared libraries')
        write(run('info sharedlibrary'))
        write('--- mappings')
        write(run('info proc mappings'))
    write('--- all threads')
    write(run('thread apply all bt 40'))


def light_dump(name):
    count = state['light'].get(name, 0) + 1
    state['light'][name] = count
    thread = gdb.selected_thread()
    write('\n--- %s marker %s #%d; thread %s (LWP %s)' % (
        stamp(), name, count, thread.num if thread else '?', thread.ptid[1] if thread else '?'))
    write(run('bt 16'))
    if name == '_XError':
        # XErrorEvent: type, display, resourceid, serial, then error/request/minor codes.
        write(run('x/5gx $rsi'))
    if count >= LIGHT[name]:
        for breakpoint in gdb.breakpoints() or ():
            if breakpoint.location == name:
                breakpoint.enabled = False
        write('(%s marker cap reached; disabled)' % name)


def alive():
    inferior = gdb.selected_inferior()
    return inferior.is_valid() and inferior.pid != 0


def start_failed(reason):
    # No application PID file is written, so the harness never signals or polls anything.
    # gdb exits non-zero; the job keeps its original failure either way.
    result = {'end': 'start failed', 'reason': reason, 'pid': None, 'at': stamp()}
    write(json.dumps(result))
    write_atomic('gdb-exit.json', json.dumps(result, indent=1) + '\n')
    log.close()
    gdb.execute('quit 3')
    raise SystemExit(3)


for command in ('set pagination off', 'set confirm off', 'set width 0', 'set height 0',
                'set print thread-events off', 'set breakpoint pending on',
                'set startup-with-shell off', 'set detach-on-fork on', 'set follow-fork-mode parent',
                'set follow-exec-mode same', 'set auto-load python-scripts off',
                'set auto-load gdb-scripts off', 'set debuginfod enabled off',
                'set backtrace limit 100', 'set print frame-arguments none'):
    write(run(command).strip() or command)
for name in QUIET:
    run('handle %s nostop noprint pass' % name)
for name in LOUD:
    run('handle %s stop print' % name)

write('=== %s DIAGNOSTIC ONLY: traced reproduction, not acceptance evidence' % stamp())
# Not run(): a failed start must not be swallowed into a log line and then reported as PID 0.
try:
    write(gdb.execute('starti', to_string=True) or '')
except gdb.error as error:
    start_failed('starti failed: %s' % error)
pid = gdb.selected_inferior().pid
if type(pid) is not int or pid <= 1:
    start_failed('invalid application pid %r after starti' % (pid,))
for spec in FULL + tuple(LIGHT):
    try:
        gdb.Breakpoint(spec)
    except gdb.error as error:
        write('breakpoint %s unavailable: %s' % (spec, error))
gdb.events.stop.connect(on_stop)
gdb.events.exited.connect(on_exit)
# The harness reads the real application PID from here; window ownership is checked against it.
write_atomic('gdb-app.pid', '%d\n' % pid)
write('application pid %d' % pid)

end = 'running'
while True:
    state['event'] = None
    try:
        write(run('continue'))
    except KeyboardInterrupt:
        write('gdb interrupted at %s' % stamp())
    if not alive():
        end = 'exited'
        break
    event = state['event']
    if isinstance(event, gdb.BreakpointEvent):
        names = [b.location for b in event.breakpoints]
        if names and names[0] in LIGHT:
            light_dump(names[0])
            continue
        full_dump('breakpoint ' + ','.join(names))
    elif isinstance(event, gdb.SignalEvent):
        full_dump('signal ' + event.stop_signal)
        if event.stop_signal == 'SIGSTOP':
            end = 'stopped by harness after observation'
            break
    else:
        full_dump('stop %s' % type(event).__name__)
        end = 'unexpected stop'
        break
    if state['full'] >= MAX_FULL:
        end = 'full-stop cap reached'
        break

if alive():
    run('kill')
result = {
    'end': end,
    'pid': pid,
    'exit_code': state['exit'],
    'exitcode_var': run('p $_exitcode').strip(),
    'exitsignal_var': run('p $_exitsignal').strip(),
    'full_stops': state['full'],
    'markers': state['light'],
    'at': stamp(),
}
write(json.dumps(result))
write_atomic('gdb-exit.json', json.dumps(result, indent=1) + '\n')
log.close()
