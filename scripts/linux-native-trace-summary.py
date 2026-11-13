"""DIAGNOSTIC ONLY. Summarise an `strace -f -tt -T -yy` capture of the Linux development app.

Usage: python3 -I linux-native-trace-summary.py <strace.txt> <summary.txt>

Writes a bounded summary for the application process (its first PID and every thread it
created): how it ended, which thread ended it, the last X11 socket operations, signals and
the final syscalls of each thread. The full capture is compressed next to the input
(strace.txt.gz) and the raw file is removed, so the artifact stays small.
"""
import collections
import gzip
import json
import os
import re
import shutil
import sys

LINE = re.compile(r'^(\d+)\s+(\S+)\s+(.*)$')
CLONE = re.compile(r'^(?:clone3?|vfork|fork)\((.*)\)\s+=\s+(\d+)')
RESUMED_CLONE = re.compile(r'^<\.\.\. (?:clone3?|vfork|fork) resumed>(.*)\)\s+=\s+(\d+)')
X11 = re.compile(r'X11-unix|\.X11-unix')
TAIL_PER_THREAD = 12
X11_TAIL = 80
MAIN_TAIL = 200


def summarise(lines):
    main = None
    tgid = {}
    pending = {}
    entries = []
    for raw in lines:
        match = LINE.match(raw.rstrip('\n'))
        if not match:
            continue
        tid, at, call = int(match.group(1)), match.group(2), match.group(3)
        if main is None:
            main = tid
            tgid[tid] = tid
        entries.append((tid, at, call))
        # A thread clone keeps its creator's thread group; anything else starts a new process.
        if call.startswith(('clone(', 'clone3(', 'vfork(', 'fork(')) and 'unfinished' in call:
            pending[tid] = call
        found = CLONE.match(call)
        resumed = RESUMED_CLONE.match(call)
        if found or resumed:
            text = (found.group(1) if found else pending.pop(tid, '') + resumed.group(1))
            child = int((found or resumed).group(2))
            if child > 0:
                tgid[child] = tgid.get(tid, tid) if 'CLONE_THREAD' in text else child
    group = tgid.get(main)
    own = [e for e in entries if tgid.get(e[0]) == group]
    ending = [e for e in own if e[2].startswith(('exit_group(', '+++', '--- SIG', 'kill(', 'tgkill(', 'tkill(', 'rt_sigqueueinfo('))
              or 'killed by' in e[2]]
    x11 = [e for e in own if X11.search(e[2])]
    per_thread = collections.OrderedDict()
    for entry in own:
        per_thread.setdefault(entry[0], collections.deque(maxlen=TAIL_PER_THREAD)).append(entry)
    exit_status = None
    for _, _, call in own:
        status = re.match(r'^\+\+\+ (exited with (\d+)|killed by (\S+))', call)
        if status:
            exit_status = status.group(1)
    enders = [e for e in own if e[2].startswith('exit_group(')]
    return {
        'main_pid': main,
        'threads': sorted({e[0] for e in own}),
        'other_processes': sorted({t for t, g in tgid.items() if g != group}),
        'exit_status': exit_status,
        'exit_group_threads': sorted({e[0] for e in enders}),
        'ending': ending,
        'x11': x11[-X11_TAIL:],
        'x11_count': len(x11),
        'main_tail': own[-MAIN_TAIL:],
        'per_thread': per_thread,
    }


def render(result):
    out = ['DIAGNOSTIC ONLY: strace reproduction summary, not acceptance evidence.']
    head = {k: result[k] for k in ('main_pid', 'exit_status', 'exit_group_threads', 'x11_count')}
    head['threads'] = len(result['threads'])
    head['other_processes'] = result['other_processes']
    out.append(json.dumps(head))

    def block(title, rows):
        out.append('\n## ' + title)
        out.extend('%d %s %s' % row for row in rows)

    block('Process end, signals and kill calls (application threads)', result['ending'])
    block('Last X11 socket operations (application threads)', result['x11'])
    block('Last syscalls across application threads', result['main_tail'])
    out.append('\n## Last syscalls per application thread')
    for tid, rows in result['per_thread'].items():
        out.append('-- thread %d' % tid)
        out.extend('%d %s %s' % row for row in rows)
    return '\n'.join(out) + '\n'


def main():
    source, summary = sys.argv[1], sys.argv[2]
    with open(source, errors='replace') as handle:
        result = summarise(handle)
    with open(summary, 'w') as handle:
        handle.write(render(result))
    with open(source, 'rb') as raw, gzip.open(source + '.gz', 'wb', compresslevel=6) as packed:
        shutil.copyfileobj(raw, packed)
    os.remove(source)


if __name__ == '__main__':
    main()
