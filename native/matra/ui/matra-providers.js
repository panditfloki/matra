/* Optional sources, custom endpoints and the explicit OpenCode bridge controls. */
((root) => {
  const esc = v => String(v ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const secretArg = (value, remove) => remove ? '' : (String(value || '').trim() || null);
  const defaults = {id:'',name:'',base_url:'',header_key:'Authorization',api_type:'openAICompatible',
    selected_model:'',open_code_provider_id:null,open_code_model_id:null,available_models:[],is_enabled:true,accent_color_hex:'#6366F1',icon_preset:'openai',
    usage_source:'manual',usage_preset:null,usage_url:null,usage_records_path:null,usage_model_field:null,
    usage_token_field:null,usage_model_filter:null,usage_authentication:'apiKey',usage_history:[],
    tracking_unit:'currency',monthly_budget_usd:null,current_spend_usd:null,monthly_budget_tokens_m:null,
    current_tokens_used_m:null,display_remaining:false,show_currency:false};
  const numeric = ['monthly_budget_usd','current_spend_usd','monthly_budget_tokens_m','current_tokens_used_m'];
  function draftFrom(values, previous) {
    const value = {...defaults,...previous,...values}; delete value.has_key;
    for (const key of ['name','base_url','header_key','selected_model']) value[key] = String(value[key] || '').trim();
    for (const key of numeric) value[key] = value[key] === '' || value[key] == null ? null : Number(value[key]);
    for (const key of ['usage_preset','usage_url','usage_records_path','usage_model_field','usage_token_field','usage_model_filter']) value[key] = value[key] || null;
    return value;
  }
  function validate(value) {
    if (!value.name) return 'Enter a name for this endpoint.';
    try { const u = new URL(value.base_url); if (!['http:','https:'].includes(u.protocol) || u.username || u.password) throw 0; }
    catch { return 'Enter an http or https base URL without embedded credentials.'; }
    if (numeric.some(k => value[k] != null && (!Number.isFinite(value[k]) || value[k] < 0))) return 'Budgets must be numbers of zero or more.';
    return null;
  }
  const button = (action,label,extra='') => `<button type="button" class="btn" data-source-action="${action}" ${extra}>${esc(label)}</button>`;
  function sourceHtml(info, snapshot = {}) {
    const toggle = `<input type="checkbox" data-source-toggle="${esc(info.id)}" ${info.enabled ? 'checked' : ''} aria-label="Enable ${esc(info.name)}">`;
    let html = `<div class="acct src-form" data-source="${esc(info.id)}"><div class="acct-line"><strong>${esc(info.name)}</strong>${toggle}</div>`;
    if (info.guidance) html += `<p class="cap">${esc(info.guidance)}</p>`;
    if (info.endpoint_scope !== 'none') html += `<div class="acct-sub"><label>Server address</label><span class="src-inputs"><input name="endpoint" value="${esc(info.endpoint)}" aria-label="${esc(info.name)} server address">${button('endpoint','Save address')}</span></div>`;
    if (info.accepts_secret) html += `<div class="acct-sub"><label>${esc(info.secret_label || 'API key')}</label><span class="src-inputs"><input type="password" name="secret" value="" autocomplete="off" placeholder="${info.has_stored_secret ? 'Stored in Windows Credential Manager' : 'Enter a key'}" aria-label="${esc(info.secret_label || 'API key')}">${button('key','Save key')}${info.has_stored_secret ? button('clear-key','Remove key') : ''}</span></div>`;
    if (info.has_env_secret) html += '<div class="cap">A key is available from your environment.</div>';
    html += `<div class="acct-sub"><span class="hint">${esc(snapshot.note || snapshot.status || (info.enabled ? 'Waiting for a reading' : 'Off'))}</span>${info.enabled ? button('refresh','Refresh') : ''}</div></div>`;
    return html;
  }
  function bridgeHtml(status = {}) {
    return `<div class="sec">OpenCode activity bridge</div><div class="group"><div class="item"><span>${status.installed ? 'Bridge installed' : 'Bridge not installed'}</span>${status.installed && status.owned ? button('bridge-remove','Remove') : button('bridge-install','Install')}</div><div class="item cap">Nothing is installed until you press Install. The bridge records activity metadata, without prompts or answers. Restart OpenCode after a change.</div><div class="item cap">${esc(status.location || '')}${status.opencode_found === false ? '<br>OpenCode was not found on this PC.' : ''}</div></div>`;
  }
  function mount(host, {invoke,listen,onChange = () => {}}) {
    const state = {registry:[],endpoints:[],usage:{},presets:[],bridge:null,editor:null,error:'',busy:false,loaded:false};
    const textField = (name,label,value,type='text') => `<div class="acct-sub"><label for="ep-${name}">${esc(label)}</label><input id="ep-${name}" name="${name}" type="${type}" value="${esc(value ?? '')}" ${type === 'number' ? 'min="0" step="any"' : ''}></div>`;
    const selectField = (name,label,value,choices) => `<div class="acct-sub"><label for="ep-${name}">${esc(label)}</label><select id="ep-${name}" name="${name}">${choices.map(([id,label])=>`<option value="${esc(id)}" ${value === id ? 'selected' : ''}>${esc(label)}</option>`).join('')}</select></div>`;
    function editorHtml(endpoint) {
      return `<form id="source-editor" class="group src-editor"><div class="item"><strong>${endpoint.id ? 'Edit endpoint' : 'New endpoint'}</strong>${button('cancel','Cancel')}</div><div class="acct">`
        + textField('name','Name',endpoint.name) + textField('base_url','Base URL',endpoint.base_url)
        + selectField('api_type','Protocol',endpoint.api_type,[['openAICompatible','OpenAI compatible'],['anthropic','Anthropic'],['google','Gemini']])
        + textField('header_key','API key header',endpoint.header_key) + textField('secret','API key','','password')
        + `<div class="acct-sub"><label><input name="remove_secret" type="checkbox"> Remove saved key</label><span class="hint">Empty keeps the saved key.</span></div>`
        + textField('selected_model','Model',endpoint.selected_model)
        + textField('open_code_provider_id','OpenCode provider ID',endpoint.open_code_provider_id)
        + textField('open_code_model_id','OpenCode model ID (optional)',endpoint.open_code_model_id)
        + `<div class="acct-sub"><label><input name="is_enabled" type="checkbox" ${endpoint.is_enabled ? 'checked' : ''}> Enabled</label><span class="hint">${endpoint.has_key ? 'A key is stored securely.' : ''}</span></div>`
        + selectField('usage_preset','Usage adapter',endpoint.usage_preset || '',[['','Manual / JSON mapping'],['openRouter','OpenRouter'],['litellm','LiteLLM'],['newAPI','New API'],['vllm','vLLM'],['llamaCpp','llama.cpp'],['abacus','Abacus']])
        + selectField('tracking_unit','Track',endpoint.tracking_unit,[['currency','Currency (USD)'],['tokens','Tokens (millions)']])
        + numeric.map((k,i)=>textField(k,['Monthly budget (USD)','Current spend (USD)','Monthly budget (M tokens)','Current use (M tokens)'][i],endpoint[k],'number')).join('')
        + `<details><summary>JSON usage mapping</summary>`
        + selectField('usage_source','Usage source',endpoint.usage_source,[['manual','Manual'],['jsonEndpoint','JSON endpoint']])
        + textField('usage_url','Usage URL',endpoint.usage_url) + textField('usage_records_path','Records path',endpoint.usage_records_path)
        + textField('usage_model_field','Model field',endpoint.usage_model_field) + textField('usage_token_field','Token field',endpoint.usage_token_field)
        + textField('usage_model_filter','Model filter',endpoint.usage_model_filter)
        + selectField('usage_authentication','Usage authentication',endpoint.usage_authentication,[['apiKey','API key'],['none','None']])
        + `<div class="acct-sub"><label for="mapping-file">Import mapping JSON</label><input id="mapping-file" type="file" accept=".json,application/json"></div></details>`
        + `<p class="cap">Save stores the key in Windows Credential Manager. Changing to a different server removes the old key unless you enter a new one.</p></div><div class="item src-actions">${button('test','Test & discover models')}${button('detect','Detect usage')}<button type="submit" class="btn src-primary">Save endpoint</button></div></form>`;
    }
    function render() {
      let html = '<div class="sec">Other AI sources</div>';
      if (state.error) html += `<div class="group"><div class="item cap act-error" role="alert">${esc(state.error)}</div></div>`;
      if (!state.loaded) html += '<div class="group"><div class="item cap">Loading sources…</div></div>';
      else html += `<div class="group">${state.registry.map(info=>sourceHtml(info,state.usage[info.id])).join('')}</div>`;
      html += `<div class="sec">Custom endpoints</div><div class="group"><div class="item"><span>Connect your own model server or API.</span>${button('new','Add endpoint')}</div>`;
      html += state.endpoints.map(e=>`<div class="acct" data-endpoint="${esc(e.id)}"><div class="acct-line"><strong>${esc(e.name)}</strong><span class="src-inputs"><input type="checkbox" data-custom-toggle="${esc(e.id)}" ${e.is_enabled ? 'checked' : ''} aria-label="Enable ${esc(e.name)}">${button('edit','Edit')}${button('delete','Remove')}</span></div><div class="cap src-url">${esc(e.base_url)}</div></div>`).join('') + '</div>';
      if (state.editor) html += editorHtml(state.editor);
      else html += `<div class="item"><select id="source-preset" aria-label="Endpoint preset"><option value="">Choose a preset</option>${state.presets.map((p,i)=>`<option value="${i}">${esc(p.name)}</option>`).join('')}</select>${button('scan','Find local servers')}</div>`;
      html += bridgeHtml(state.bridge);
      host.innerHTML = html;
      if (state.busy) host.querySelectorAll('button,input,select').forEach(e=>e.disabled=true);
    }
    async function load() {
      try {
        const [registry,endpoints,usage,presets,bridge] = await Promise.all(['get_provider_registry','list_custom_endpoints','get_parity_usage','get_custom_presets','get_opencode_bridge_status'].map(c=>invoke(c)));
        Object.assign(state,{registry,endpoints,usage,presets,bridge,loaded:true});
        onChange({registry,endpoints}); api.synced=true;
      } catch(e) { state.error=String(e?.message || e); }
      render();
    }
    async function action(fn, reload=true) {
      if (state.busy) return;
      state.busy=true; state.error='';
      host.querySelectorAll('button').forEach(e=>e.disabled=true);
      try { await fn(); if (reload) await load(); }
      catch(e) { state.error=String(e?.message || e); }
      finally { state.busy=false; render(); }
    }
    function draft() {
      const form=host.querySelector('#source-editor'), values=Object.fromEntries(new FormData(form));
      values.is_enabled=form.elements.is_enabled.checked;
      const secret=secretArg(values.secret,form.elements.remove_secret.checked);
      delete values.secret; delete values.remove_secret;
      const endpoint=draftFrom(values,state.editor); const error=validate(endpoint);
      if (error) throw new Error(error);
      state.editor={...endpoint,has_key:state.editor?.has_key};
      return {endpoint,secret};
    }
    host.addEventListener('submit',e=>{
      if (e.target.id!=='source-editor') return; e.preventDefault();
      let args; try { args=draft(); } catch(error) { state.error=error.message; render(); return; }
      action(async()=>{await invoke('save_custom_endpoint',args); state.editor=null;});
    });
    host.addEventListener('click',e=>{
      const b=e.target.closest('[data-source-action]'); if(!b || state.busy) return;
      const cmd=b.dataset.sourceAction, source=b.closest('[data-source]'), item=b.closest('[data-endpoint]');
      const id=source?.dataset.source;
      if(cmd==='new'){state.editor={...defaults};state.error='';render();return;}
      if(cmd==='cancel'){state.editor=null;render();return;}
      if(cmd==='edit'){state.editor={...state.endpoints.find(v=>v.id===item.dataset.endpoint)};state.error='';render();return;}
      let args;
      if(cmd==='test'||cmd==='detect') { try {args=draft();} catch(error){state.error=error.message;render();return;} }
      const value=source?.querySelector('[name="endpoint"]')?.value;
      const key=source?.querySelector('[name="secret"]')?.value;
      action(async()=>{
        if(cmd==='endpoint') await invoke('set_parity_endpoint',{id,endpoint:value || null});
        if(cmd==='key') {if(!key?.trim()) throw new Error('Enter a key first.');await invoke('set_provider_secret',{id,secret:key});}
        if(cmd==='clear-key') await invoke('clear_provider_secret',{id});
        if(cmd==='refresh') await invoke('refresh_parity_provider',{id});
        if(cmd==='delete') await invoke('delete_custom_endpoint',{id:item.dataset.endpoint});
        if(cmd==='bridge-install') await invoke('install_opencode_bridge');
        if(cmd==='bridge-remove') await invoke('remove_opencode_bridge');
        if(cmd==='test') { const r=await invoke('test_custom_endpoint',args); state.error=r.error || `${r.health}. ${r.models?.length || 0} models found.`; if(r.models?.length) {state.editor.available_models=r.models;if(!state.editor.selected_model)state.editor.selected_model=r.models[0];} }
        if(cmd==='detect') { const r=await invoke('detect_custom_usage',args); if(r.preset) state.editor.usage_preset=r.preset;state.error=`Usage detection: ${r.result}`; }
        if(cmd==='scan') { const found=await invoke('scan_local_endpoints'); state.presets=found.concat(state.presets);state.error=found.length ? `${found.length} local servers found. Choose one from the presets.` : 'No responding local server found.'; }
      },cmd!=='test'&&cmd!=='detect'&&cmd!=='scan');
    });
    host.addEventListener('change',e=>{
      const t=e.target;
      if(t.dataset.sourceToggle) return action(()=>invoke('set_parity_provider',{id:t.dataset.sourceToggle,on:t.checked}));
      if(t.dataset.customToggle) return action(()=>invoke('set_parity_provider',{id:'custom:'+t.dataset.customToggle,on:t.checked}));
      if(t.id==='source-preset' && t.value!=='') {const p=state.presets[Number(t.value)];state.editor={...defaults,...p,id:'',selected_model:p.default_model || '',usage_preset:null};render();}
      if(t.id==='mapping-file'&&t.files?.[0]) {
        const file=t.files[0]; if(file.size>65536){state.error='Mapping files must be 64 KiB or smaller.';render();return;}
        let base;try{base=draft().endpoint.base_url;}catch(error){state.error=error.message;render();return;}
        action(async()=>{const mapping=await invoke('import_custom_usage_mapping',{base,text:await file.text()});state.editor={...state.editor,...mapping,usage_source:'jsonEndpoint'};},false);
      }
    });
    if(listen) listen('parity_usage',e=>{state.usage=e.payload || {};if(!state.editor && !state.busy && !host.contains(document.activeElement)) render();}).catch(()=>{});
    const api={load,synced:false,state}; render(); return api;
  }
  root.MatraProviders={mount,sourceHtml,bridgeHtml,draftFrom,validate,secretArg};
  if(typeof module!=='undefined') module.exports=root.MatraProviders;
})(typeof window==='undefined'?globalThis:window);
