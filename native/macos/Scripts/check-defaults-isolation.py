#!/usr/bin/env python3
"""Compile and prove the app's memory preferences without launching an app/XCTest.

Pass an explicit compiler/SDK and a disposable workspace. The executable runs
with preferences-daemon access denied and writes allowed only in workspace/run.
No HOME/CFFIXED_USER_HOME override or preference-file cleanup is involved.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workspace', type=Path, required=True)
    parser.add_argument('--swiftc', type=Path, required=True)
    parser.add_argument('--sdk', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    workspace = args.workspace.resolve()
    workspace.mkdir(parents=True, exist_ok=True)
    paths = {name: workspace / name for name in ['tmp', 'modules', 'run']}
    for path in paths.values():
        path.mkdir(parents=True, exist_ok=True)
        if not path.resolve().is_relative_to(workspace):
            raise RuntimeError('Fixture output symlink leaves workspace: ' + str(path))
    for name in ['defaults-isolation', 'defaults.sb', 'results.json', 'receipt.json', 'denied-canary']:
        if not (workspace / name).resolve().is_relative_to(workspace):
            raise RuntimeError('Fixture output symlink leaves workspace: ' + name)
    # App-owned settings must enter the façade. This also catches a future raw
    # @AppStorage binding or a test's innocent-looking named Foundation suite.
    violations = []
    checked = 0
    for group in ['Sources', 'Tests']:
        for path in (root / group).rglob('*.swift'):
            if path.name == 'MatraDefaults.swift':
                continue
            checked += 1
            for number, line in enumerate(path.read_text().splitlines(), 1):
                code = line.split('//', 1)[0]
                if re.search(r'\b(?:UserDefaults|NSUserDefaults|CFPreferences\w*)\b|@AppStorage\s*\(', code):
                    violations.append(f'{path.relative_to(root)}:{number}: {line.strip()}')
                # AppKit autosave writes through cfprefsd. The dashboard is the one
                # reviewed caller, gated by Runtime.usesInMemoryDefaults.
                if (re.search(r'\b(?:setFrameAutosaveName|saveFrame|autosaveName|autosaveTableColumns|'
                              r'autosaveExpandedItems)\b', code)
                        and path.name != 'DashboardWindowController.swift'):
                    violations.append(f'{path.relative_to(root)}:{number}: {line.strip()}')
    if violations:
        raise RuntimeError('Preferences boundary bypass:\n' + '\n'.join(violations))

    sources = [root / 'Sources/App' / (name + '.swift')
               for name in ['MatraStorage', 'Runtime', 'MatraDefaults']]
    fixture = root / 'Scripts/Fixtures/DefaultsIsolation'
    binary = workspace / 'defaults-isolation'
    env = dict(os.environ, TMPDIR=str(paths['tmp']) + '/',
               CLANG_MODULE_CACHE_PATH=str(paths['modules']))
    # swiftc dispatches by argv[0]; resolving its symlink can invoke swift-frontend.
    args.swiftc.resolve(strict=True)
    flags = [str(args.swiftc.absolute()), '-swift-version', '5',
             '-target', platform.machine() + '-apple-macosx15.0',
             '-sdk', str(args.sdk.resolve(strict=True)),
             '-module-cache-path', str(paths['modules'])]
    logs = []

    def run(command, environment):
        result = subprocess.run([str(item) for item in command], env=environment,
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        logs.append({'command': [str(item) for item in command], 'exitCode': result.returncode,
                     'output': result.stdout})
        (workspace / 'results.json').write_text(json.dumps(logs, indent=2) + '\n')
        print(result.stdout, end='', flush=True)
        if result.returncode:
            raise SystemExit(result.returncode)

    run(flags + ['-import-objc-header', fixture / 'bridge.h'] + sources
        + [fixture / 'main.swift', '-o', binary], env)
    # Check the SwiftUI adapter's types without opening windows or installing an
    # app. Its XCTest behavior is checked later by the coordinator's hosted run.
    run(flags + ['-typecheck'] + sources + [root / 'Sources/App/MatraAppStorage.swift'], env)
    profile = workspace / 'defaults.sb'
    profile.write_text('(version 1)\n(allow default)\n(deny network*)\n'
                       '(deny file-write*)\n'
                       '(allow file-write* (subpath ' + json.dumps(str(paths['run'])) + '))\n'
                       '(deny mach-lookup (global-name-regex "^com[.]apple[.]cfprefsd[.]"))\n')
    canary = workspace / 'denied-canary'
    if canary.exists():
        raise RuntimeError('Choose a fresh workspace: denial canary already exists')
    isolated_env = dict(env, MATRA_IN_MEMORY_DEFAULTS='1', TMPDIR=str(paths['run']) + '/',
                        MATRA_ISOLATION_DENIED_PATH=str(canary))
    run(['/usr/bin/sandbox-exec', '-f', profile, binary], isolated_env)
    receipt = {'boundaryFilesChecked': checked, 'rawDefaultsBypasses': 0,
               'hostedTestsRun': False, 'binarySHA256': hashlib.sha256(binary.read_bytes()).hexdigest(),
               'sourceSHA256': {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
                                for path in sources + [fixture / 'main.swift', fixture / 'bridge.h',
                                                       root / 'Sources/App/MatraAppStorage.swift']}}
    (workspace / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(f'Preferences boundary: {checked} source/test files checked; zero bypasses')


if __name__ == '__main__':
    main()
