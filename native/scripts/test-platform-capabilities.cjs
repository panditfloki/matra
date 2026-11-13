const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../matra/ui/platform-capabilities.js'), 'utf8');
async function environment(capabilities) {
  const calls = [];
  const info = {label:'Fixture', limitations:[], capabilities};
  const core = Object.freeze({invoke: async command => {calls.push(command); return command==='get_platform_info'?info:'accepted';}});
  const window = {__TAURI_INTERNALS__:{}, __TAURI__:{core}};
  vm.runInNewContext(source, {window, document:{readyState:'complete',body:{},querySelectorAll:()=>[],getElementById:()=>null}, MutationObserver:class {observe(){}}});
  await window.matraPlatformReady;
  return {invoke:window.__TAURI__.core.invoke,calls};
}
(async()=>{
  const x11=await environment({notch_drag:true,updates:false,hooks:true,autostart:true});
  assert.equal(await x11.invoke('begin_move'), 'accepted');
  assert.equal(await x11.invoke('set_hooks_installed'), 'accepted');
  assert.equal(await x11.invoke('set_autostart'), 'accepted');
  await assert.rejects(x11.invoke('check_for_update'), /disabled/);
  await assert.rejects(x11.invoke('set_automatic_updates'), /disabled/);
  assert.ok(!x11.calls.includes('check_for_update'));
  const wayland=await environment({notch_drag:false,absolute_window_placement:false,terminal_focus:false});
  for(const cmd of ['begin_move','drag_begin','set_notch_monitor','set_notch_edge','reset_notch_position','focus_session']) {
    await assert.rejects(wayland.invoke(cmd)); assert.ok(!wayland.calls.includes(cmd));
  }
  const fixture=await environment({hooks:false,autostart:false,secure_storage:false});
  for(const cmd of ['set_hooks_installed','set_autostart','set_provider_secret'])await assert.rejects(fixture.invoke(cmd));
  console.log('PASS: frozen Tauri module, X11 carry, explicit development integrations, production-update and Wayland gates.');
})().catch(e=>{console.error(e);process.exitCode=1;});
