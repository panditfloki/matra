// Packaging contract only. This is not an install drill or antivirus clearance.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const native = path.resolve(__dirname, '..');
const read = name => fs.readFileSync(path.join(native, name), 'utf8');
const hooks = read('matra/installer/hooks.nsh');
const helper = read('matra-hook/src/setup.rs');
const main = read('matra/src/main.rs');
const audit = read('setup_audit.rs');

assert.match(hooks, /!ifmacrondef CheckIfAppIsRunning[\s\S]*?!error/);
assert.match(hooks, /!macroundef CheckIfAppIsRunning/);
const fallback = hooks.match(/!macro CheckIfAppIsRunning[^\r\n]*([\s\S]*?)!macroend/)[1];
assert.match(fallback, /!insertmacro MATRA_PREPARE/);
assert.doesNotMatch(hooks, /::KillProcess|taskkill|TerminateProcess|ExecutionPolicy/i);
assert.doesNotMatch(helper, /\bTerminateProcess\b|\bPROCESS_TERMINATE\b/);
assert.match(helper, /GetWindowThreadProcessId/);
assert.match(helper, /owner == request.pid/);
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
  assert(installer.indexOf('utils.nsh"') < installer.indexOf('hooks.nsh"'));
  for (const phase of ['PREINSTALL', 'PREUNINSTALL']) {
    const start = installer.indexOf(`!ifmacrodef NSIS_HOOK_${phase}`);
    assert(start >= 0);
    const next = installer.indexOf('!insertmacro CheckIfAppIsRunning', start);
    assert(next > start);
  }
}
console.log('PASS: graceful-only installer contract, preflight order, diagnostic boundary');
