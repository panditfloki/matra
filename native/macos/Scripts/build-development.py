#!/usr/bin/env python3
"""Build the existing native Mac application as an isolated 0.2.0 development app.

All generated state is placed under --workspace. XcodeGen and the pre-resolved
Swift package cache are explicit inputs. No installed app or update feed changes.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET

VERSION = '0.2.0'
BUILD = '2001'
BUNDLE = 'com.dydxfx.matra.mac.development'


def run(command, *, env, cwd, log=None):
    print('+ ' + ' '.join(map(str, command)), flush=True)
    if log:
        with Path(log).open('w') as stream:
            result = subprocess.run(list(map(str, command)), cwd=cwd, env=env,
                                    stdout=stream, stderr=subprocess.STDOUT)
        if result.returncode:
            print(Path(log).read_text(errors='replace')[-18000:], file=sys.stderr)
            raise subprocess.CalledProcessError(result.returncode, command)
    else:
        subprocess.run(list(map(str, command)), cwd=cwd, env=env, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workspace', type=Path, required=True)
    parser.add_argument('--xcodegen', type=Path, required=True)
    parser.add_argument('--packages', type=Path, required=True, help='Read-only existing resolved Swift package cache')
    parser.add_argument('--arch', choices=['arm64', 'x86_64'], default='arm64')
    parser.add_argument('--tests', action='store_true')
    parser.add_argument('--test-only', action='store_true')
    parser.add_argument('--package', action='store_true')
    parser.add_argument('--only-testing', action='append', default=[], help='Xcode test selector; repeatable')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    workspace = args.workspace.resolve()
    workspace.mkdir(parents=True, exist_ok=True)
    paths = {name: workspace / name for name in ['tmp', 'runtime', 'modules', 'package-cache', 'packages', 'derived', 'logs', 'artifacts', 'generated']}
    for path in paths.values():
        path.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, TMPDIR=str(paths['tmp']) + '/',
               CFFIXED_USER_HOME=str(paths['runtime']), XDG_CACHE_HOME=str(paths['package-cache']),
               CLANG_MODULE_CACHE_PATH=str(paths['modules']),
               SWIFTPM_MODULECACHE_OVERRIDE=str(paths['modules']),
               MATRA_TEST_WORKSPACE=str(workspace), MATRA_IN_MEMORY_DEFAULTS="1")
    # Copy before SwiftPM can mutate the cache. Rewrite only copied absolute paths.
    source_packages = args.packages.resolve(strict=True)
    if source_packages != paths['packages'].resolve() and not (paths['packages'] / 'workspace-state.json').exists():
        shutil.copytree(source_packages, paths['packages'], dirs_exist_ok=True, symlinks=True)
        state = paths['packages'] / 'workspace-state.json'
        if state.exists():
            state.write_text(state.read_text().replace(str(source_packages), str(paths['packages'])))
    for link in paths['packages'].rglob('*'):
        if link.is_symlink() and not link.resolve().is_relative_to(paths['packages']):
            raise RuntimeError('Package symlink leaves build workspace: ' + str(link))
    spec = json.loads(subprocess.check_output(['/usr/bin/ruby', '-rjson', '-ryaml', '-e',
        'puts JSON.generate(YAML.load_file(ARGV[0]))', str(root / 'project.yml')], env=env))
    spec['settings']['base'].update(MARKETING_VERSION=VERSION, CURRENT_PROJECT_VERSION=BUILD)
    target = spec['targets']['Matra']
    target['settings']['base']['PRODUCT_BUNDLE_IDENTIFIER'] = BUNDLE
    target['info']['path'] = str(paths['generated'] / 'Info.plist')
    props = target['info']['properties']
    props.update(CFBundleName='Mātrā Development', CFBundleDisplayName='Mātrā Development',
                 SUBundleName='Mātrā Development', SUEnableAutomaticChecks=False, SUAutomaticallyUpdate=False)
    props.pop('SUFeedURL', None)
    props.pop('SUPublicEDKey', None)
    spec['targets']['MatraTests']['settings']['base']['PRODUCT_BUNDLE_IDENTIFIER'] = BUNDLE + '.tests'
    spec_path = paths['generated'] / 'project.json'
    spec_path.write_text(json.dumps(spec, indent=2) + '\n')
    run([args.xcodegen.resolve(strict=True), 'generate', '--spec', spec_path,
         '--project-root', root, '--project', root, '--no-env'], env=env, cwd=root)
    project = root / 'Matra.xcodeproj'
    resolved = project / 'project.xcworkspace/xcshareddata/swiftpm/Package.resolved'
    resolved.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(root / 'Package.resolved', resolved)
    scheme = project / 'xcshareddata/xcschemes/Matra.xcscheme'
    document = ET.parse(scheme)
    action = document.getroot().find('TestAction')
    action.set('shouldUseLaunchSchemeArgsEnv', 'NO')
    variables = ET.SubElement(action, 'EnvironmentVariables')
    for key in ['TMPDIR', 'CFFIXED_USER_HOME', 'MATRA_TEST_WORKSPACE', 'MATRA_IN_MEMORY_DEFAULTS']:
        env['TEST_RUNNER_' + key] = env[key]
        for name in (key, 'TEST_RUNNER_' + key):
            ET.SubElement(variables, 'EnvironmentVariable', key=name, value=env[key], isEnabled='YES')
    document.write(scheme, encoding='utf-8', xml_declaration=True)
    common = ['/usr/bin/xcodebuild', '-quiet', '-project', project, '-scheme', 'Matra',
              '-destination', 'platform=macOS,arch=' + args.arch,
              '-derivedDataPath', paths['derived'], '-clonedSourcePackagesDirPath', paths['packages'],
              '-packageCachePath', paths['package-cache'], '-disablePackageRepositoryCache',
              '-onlyUsePackageVersionsFromResolvedFile', '-skipPackageUpdates', '-jobs', '4',
              'CODE_SIGN_IDENTITY=-', 'DEVELOPMENT_TEAM=', 'CODE_SIGN_STYLE=Automatic',
              'ENABLE_HARDENED_RUNTIME=NO', 'CODE_SIGN_INJECT_BASE_ENTITLEMENTS=NO',
              'CLANG_MODULE_CACHE_PATH=' + str(paths['modules']), 'ARCHS=' + args.arch,
              'ONLY_ACTIVE_ARCH=YES']
    if args.tests or args.test_only:
        run_name = 'Matra-Development-Tests-' + datetime.now(timezone.utc).strftime('%Y%m%d-%H%M%S')
        result_bundle = paths['artifacts'] / (run_name + '.xcresult')
        if result_bundle.exists():
            parser.error('Test result already exists; choose a fresh workspace for a new evidence run')
        run(common + ['-configuration', 'Debug', '-parallel-testing-enabled', 'NO',
                      '-resultBundlePath', result_bundle] + ['-only-testing:' + name for name in args.only_testing] + ['test'],
            env=env, cwd=root, log=paths['logs'] / (run_name + '.log'))
    if args.test_only:
        return
    run(common + ['-configuration', 'Release', 'build'], env=env, cwd=root, log=paths['logs'] / 'build.log')
    app = paths['artifacts'] / 'Matra Development.app'
    if app.exists():
        parser.error('Staged app already exists; choose a fresh workspace')
    run(['/usr/bin/ditto', paths['derived'] / 'Build/Products/Release/Matra.app', app], env=env, cwd=root)
    run(['/usr/bin/codesign', '--force', '--deep', '--sign', '-', '--options', '0', app], env=env, cwd=root)
    run(['/usr/bin/codesign', '--verify', '--deep', '--strict', app], env=env, cwd=root)
    info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
    assert info['CFBundleIdentifier'] == BUNDLE
    assert info['CFBundleShortVersionString'] == VERSION and info['CFBundleVersion'] == BUILD
    assert 'SUFeedURL' not in info and 'SUPublicEDKey' not in info
    assert info['SUEnableAutomaticChecks'] is False
    run([sys.executable, root / 'Scripts/verify-package-launch.py', app,
         paths['artifacts'] / 'loader-check', '--arch', args.arch], env=env, cwd=root)
    outputs = []
    if args.package:
        stem = f'Matra-{VERSION}-macOS-{args.arch}-development'
        archive = paths['artifacts'] / (stem + '.zip')
        run(['/usr/bin/ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', app, archive], env=env, cwd=root)
        outputs.append(archive)
        stage = paths['artifacts'] / 'dmg-stage'
        stage.mkdir()
        run(['/usr/bin/ditto', app, stage / app.name], env=env, cwd=root)
        (stage / 'Read Me.txt').write_text('Mātrā 0.2.0 development\n\nRun Matra Development.app from this folder or copy it to a separate development folder.\nIt has separate settings and storage. Your installed Mātrā is unchanged.\nAutomatic updates are disabled for this development channel.\nThis package uses local ad-hoc signing, not Apple notarization.\n')
        dmg = paths['artifacts'] / (stem + '.dmg')
        run(['/usr/bin/hdiutil', 'create', '-volname', 'Mātrā Development', '-srcfolder', stage,
             '-format', 'UDZO', '-ov', dmg], env=env, cwd=root, log=paths['logs'] / 'dmg.log')
        run(['/usr/bin/hdiutil', 'verify', dmg], env=env, cwd=root)
        outputs.append(dmg)
    manifest = {'version': VERSION, 'build': BUILD, 'bundle_id': BUNDLE, 'architecture': args.arch,
                'native_swift_ui': True, 'automatic_updates': False,
                'files': [{'name': p.name, 'bytes': p.stat().st_size, 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()} for p in outputs]}
    (paths['artifacts'] / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps(manifest, indent=2))


if __name__ == '__main__':
    main()
