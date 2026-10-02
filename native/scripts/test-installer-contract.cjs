// Packaging contract only. This is not an install drill or antivirus clearance.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const native = path.resolve(__dirname, '..');
const read = name => fs.readFileSync(path.join(native, name), 'utf8');
const hooks = read('matra/installer/hooks.nsh');
const helper = read('matra-hook/src/setup.rs');
const main = read('matra/src/main.rs');
const audit = read('setup_audit.rs');
const repo = path.dirname(native);
const bundle = JSON.parse(read('matra/tauri.bundle.conf.json'));
const share = fs.readFileSync(path.join(repo, 'scripts/prepare-share-folder.ps1'), 'utf8');
const workflow = fs.readFileSync(path.join(repo, '.github/workflows/windows-package.yml'), 'utf8');

for (const name of ['LICENSE.txt', 'THIRD-PARTY-NOTICES.txt', 'PROVIDER-NOTICES.txt']) {
  const resource = Object.entries(bundle.bundle.resources).find(([, destination]) => destination === name);
  assert(resource, `the bundle contains ${name}`);
  assert(fs.existsSync(path.resolve(native, 'matra', resource[0])), `${name} source exists`);
}
assert.match(share, /native\\matra\\glyphs\\NOTICE\.md/);
assert.doesNotMatch(share, /native\\codenotch\\glyphs/);
assert.match(workflow, /'test-codex-details\.cjs'/);

// Evaluate the actual workflow expressions for Windows/Mac release and dispatch scenarios.
const packageGuard = workflow.match(/  package:\r?\n    if: >-\r?\n([\s\S]*?)\r?\n    permissions:/)[1];
const attachGuard = workflow.match(/      - name: Attach installer to release\r?\n        if: >-\r?\n([\s\S]*?)\r?\n        env:/)[1];
function guards(eventName, tag, dispatchTag = '') {
  const context = {
    github: { repository: 'panditfloki/matra', event_name: eventName, event: { release: { tag_name: tag } } },
    inputs: { release_tag: dispatchTag },
    startsWith: (value, prefix) => String(value || '').toLowerCase().startsWith(prefix.toLowerCase()),
    contains: (value, part) => String(value || '').toLowerCase().includes(part.toLowerCase())
  };
  return [vm.runInNewContext(packageGuard, context), vm.runInNewContext(attachGuard, context)];
}
assert.deepEqual(guards('release', 'v1.8.5-macos-beta.1'), [false, false]);
assert.deepEqual(guards('release', 'v1.8.6'), [true, true]);
assert.deepEqual(guards('workflow_dispatch', '', 'v1.8.5-macos-beta.1'), [false, false]);
assert.deepEqual(guards('workflow_dispatch', '', 'v1.8.6'), [true, true]);
assert.deepEqual(guards('workflow_dispatch', ''), [true, false]);
assert.deepEqual(guards('push', ''), [true, false]);

assert.match(hooks, /!ifmacrondef CheckIfAppIsRunning[\s\S]*?!error/);
assert.match(hooks, /!macroundef CheckIfAppIsRunning/);
const fallback = hooks.match(/!macro CheckIfAppIsRunning[^\r\n]*([\s\S]*?)!macroend/)[1];
assert.match(fallback, /!insertmacro MATRA_PREPARE/);
assert.doesNotMatch(hooks, /::KillProcess|taskkill|TerminateProcess|ExecutionPolicy/i);
assert.doesNotMatch(helper, /\bTerminateProcess\b|\bPROCESS_TERMINATE\b/);
assert.match(helper, /GetWindowThreadProcessId/);
assert.match(helper, /owner == request.pid/);
assert.match(helper, /request_close_with_class\(pid, "Tauri Window"\)/);
assert.match(helper, /GetClassNameW\(hwnd, &mut class\)/);
assert.match(helper, /String::from_utf16_lossy[\s\S]*?\.eq_ignore_ascii_case\(request\.application_class\)/);
assert.doesNotMatch(helper, /IsWindowVisible/); // A saved hidden notch still has to close.
assert.match(helper, /if !request\.matched/); // Unknown runtime windows fail without a broadcast.
assert.match(helper, /PostMessageW\(hwnd, WM_CLOSE/);
assert.match(helper, /wait_for_exit\(process\.0, remaining\)\?/);
assert.match(main, /window\.label\(\) == "notch"[\s\S]*?CloseRequested[\s\S]*?window\.app_handle\(\)\.exit\(0\)/);
const migrate = main.slice(main.indexOf('fn migrate_setup()'), main.indexOf('const CONSOLE_CMDS'));
assert(migrate.indexOf('hooks_install::prepare_migration()?') < migrate.indexOf('hooks.apply()'));
assert(migrate.indexOf('autostart::prepare_migration()?') < migrate.indexOf('hooks.apply()'));
assert(migrate.indexOf('startup.validate()?') < migrate.indexOf('hooks.apply()'));
assert.match(audit, /setup-events\.jsonl/);
assert.match(audit, /is_ascii_alphanumeric/);

// When built, also check the pinned template's include and invocation order.
if (process.argv[2]) {
  const installer = fs.readFileSync(process.argv[2], 'utf8');
  const normalized = value => value.replaceAll('/', '\\').toLowerCase();
  assert(normalized(installer).includes(normalized(path.join(native, 'matra/installer/hooks.nsh'))), 'the generated installer uses this checkout\'s hooks');
  const meta = JSON.parse(read('matra/tauri.conf.json'));
  assert(installer.includes(`!define VERSION "${meta.version}"`), 'the generated installer matches the app version');
  for (const name of ['LICENSE.txt', 'THIRD-PARTY-NOTICES.txt', 'PROVIDER-NOTICES.txt']) assert(installer.includes(name), `${name} is included in the generated installer`);
  assert(installer.indexOf('utils.nsh"') < installer.indexOf('hooks.nsh"'));
  for (const phase of ['PREINSTALL', 'PREUNINSTALL']) {
    const start = installer.indexOf(`!ifmacrodef NSIS_HOOK_${phase}`);
    assert(start >= 0);
    const next = installer.indexOf('!insertmacro CheckIfAppIsRunning', start);
    assert(next > start);
  }
}
console.log('PASS: graceful-only installer contract, preflight order, notices, Windows/Mac release boundary');
