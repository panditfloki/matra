// Test-only bridge. No native IPC, credentials, downloads or installer execution.
require('./branding-preview-preload.cjs');
const original=window.__TAURI__.core.invoke;
const listeners=new Map();
window.__test={calls:[],emit(name,payload){for(const f of listeners.get(name)||[])f({payload});}};
window.__TAURI__.event.listen=async(name,handler)=>{if(!listeners.has(name))listeners.set(name,[]);listeners.get(name).push(handler);return ()=>{};};
window.__TAURI__.core.invoke=async(name,args)=>{
  window.__test.calls.push(name);
  if(name==='check_for_update'){window.__test.emit('update_state',{phase:'available',available:'1.8.2'});return;}
  if(name==='download_update'){window.__test.emit('update_state',{phase:'downloading',downloaded:25,total:100});return;}
  if(name==='install_update'){window.__test.emit('update_state',{phase:'installing'});return;}
  if(name==='get_state')return {sessions:[],agg:'idle',lang_resolved:'en'};
  if(name==='get_activity')return [];
  if(['get_usage','get_codex','get_cursor','get_grok','get_glm','get_antigravity'].includes(name))return {status:'absent',windows:[],fetched_at:0};
  if(name==='get_claude_auth')return {busy:false};
  if(name==='get_update_state')return {phase:'idle'};
  try{return await original(name,args);}catch{return null;}
};
