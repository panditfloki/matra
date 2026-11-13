// Exact settings/editor regressions with shipped controllers and synthetic IPC. No real keys or native submissions.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const puppeteer=require(process.env.MATRA_PUPPETEER_PATH||'puppeteer');
const repo=path.resolve(__dirname,'../..'),ui=path.join(repo,'native/matra/ui'),out=path.join(repo,'.presentation-validation/p2-fixes/browser');
for(const n of ['','tmp','cache','profile'])fs.mkdirSync(path.join(out,n),{recursive:true});
assert.ok(fs.realpathSync(out).startsWith(fs.realpathSync(repo)+path.sep));
const checks=[],errors=[],network=[],check=async(name,fn)=>{await fn();checks.push(name);};
const iconPath=path.join(out,'fixture-icon.png');fs.writeFileSync(iconPath,Buffer.from(require('../matra/fixtures/presentation.json').icon.split(',')[1],'base64'));
const badIconPath=path.join(out,'invalid-icon.png');fs.writeFileSync(badIconPath,Buffer.from('synthetic invalid PNG'));
(async()=>{
 const browser=await puppeteer.launch({executablePath:process.env.MATRA_CHROME_PATH,headless:true,userDataDir:path.join(out,'profile'),env:{...process.env,TMPDIR:path.join(out,'tmp'),XDG_CACHE_HOME:path.join(out,'cache')},args:['--disable-background-networking','--disable-component-update','--no-first-run','--no-default-browser-check','--disable-crash-reporter','--disable-breakpad','--disable-sync','--disable-features=MediaRouter']});
 try{
  const page=await browser.newPage();page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));await page.setRequestInterception(true);page.on('request',r=>{if(/^https?:/.test(r.url())){network.push(r.url());r.abort();}else r.continue();});
  await page.setViewport({width:1000,height:1100});await page.setContent('<style>button,input,select{margin:5px}.switch{min-width:36px;min-height:22px}</style><main id="controls"></main><main id="sources"></main>');
  const settings=fs.readFileSync(path.join(ui,'settings.html'),'utf8');
  await page.evaluate(html=>{const doc=new DOMParser().parseFromString(html,'text/html');for(const id of ['seg-size','cap-size','sw-custom-scale','custom-scale-row','parity-status','account-presentation','sw-fullscreen','fullscreen-cap'])document.querySelector('#controls').append(doc.getElementById(id).cloneNode(true));},settings);
  for(const n of ['matra-parity.js','matra-accounts.js'])await page.addScriptTag({path:path.join(ui,n)});
  await page.evaluate(()=>{
   window.fixtureSettings=MatraParity.normalize({uses_custom_scale:true,custom_scale:1});window.fixturePreset=1;window.fixtureCalls=[];window.listeners={};window.failScale=false;
   window.__TAURI__={core:{invoke:async(name,args)=>{
    fixtureCalls.push({name,args});if(name==='get_parity_settings')return fixtureSettings;
    if(name==='get_scale')return fixtureSettings.uses_custom_scale?fixtureSettings.custom_scale:fixturePreset;
    if(name==='set_parity_settings'){fixtureSettings=args.settings;return fixtureSettings;}
    if(name==='set_scale'){if(failScale)throw Error('Synthetic scale save refusal');fixturePreset=args.scale;fixtureSettings={...fixtureSettings,uses_custom_scale:false};listeners.parity_settings?.({payload:fixtureSettings});return fixturePreset;}
    if(name==='get_usage')return{status:'absent',windows:[]};if(name==='get_platform_info')return{capabilities:{fullscreen_detection:true}};return null;
   }},event:{listen:async(name,fn)=>{listeners[name]=fn;return()=>{};}}};
   window.invoke=(name,args)=>__TAURI__.core.invoke(name,args);window.call=(name,args,fallback)=>invoke(name,args).catch(()=>fallback);
   window.ui=(key,fallback)=>fallback;window.toast=()=>{};window.strip=()=>{};window.errText=String;window.renderAccounts=()=>{};
  });
  const parityStart=settings.indexOf('let ringOrderMode ='),parityEnd=settings.indexOf('function publishSources()',parityStart);
  const sizeStart=settings.indexOf('const SIZE_WHY ='),sizeEnd=settings.indexOf('/* ---- Appearance: edge',sizeStart);
  assert.ok(parityStart>=0&&parityEnd>parityStart&&sizeStart>=0&&sizeEnd>sizeStart);
  // Execute the actual inline preset handler and callback, not a reimplementation of their guards.
  await page.addScriptTag({content:settings.slice(parityStart,parityEnd)+'\n'+settings.slice(sizeStart,sizeEnd)});
  await page.addScriptTag({path:path.join(ui,'matra-parity-settings.js')});await page.waitForFunction(()=>!document.querySelector('#sw-custom-scale').disabled);
  for(const scale of [.8,1,1.25])await check(`equal ${scale} preset exits custom mode and keeps the custom value`,async()=>{
   await page.evaluate(s=>{fixtureSettings={...fixtureSettings,uses_custom_scale:true,custom_scale:s};listeners.parity_settings({payload:fixtureSettings});},scale);
   assert.equal(await page.$$eval('#seg-size [aria-pressed="true"]',els=>els.length),0);
   const before=await page.evaluate(()=>fixtureCalls.filter(c=>c.name==='set_scale').length);
   await page.click(`#seg-size button[data-v="${scale}"]`);await page.waitForFunction(()=>!fixtureSettings.uses_custom_scale&&!sizeBusy);
   assert.equal(await page.evaluate(()=>fixtureCalls.filter(c=>c.name==='set_scale').length),before+1);
   assert.equal(await page.evaluate(()=>fixtureSettings.custom_scale),scale);assert.equal(await page.$eval(`#seg-size button[data-v="${scale}"]`,e=>e.getAttribute('aria-pressed')),'true');
   assert.equal(await page.$eval('#sw-custom-scale',e=>e.getAttribute('aria-checked')),'false');
   await page.click(`#seg-size button[data-v="${scale}"]`);assert.equal(await page.evaluate(()=>fixtureCalls.filter(c=>c.name==='set_scale').length),before+1);
  });
  await check('failed preset save keeps active custom mode and does not mark a preset selected',async()=>{
   await page.evaluate(()=>{fixtureSettings={...fixtureSettings,uses_custom_scale:true,custom_scale:1};listeners.parity_settings({payload:fixtureSettings});failScale=true;});
   await page.click('#seg-size button[data-v="1"]');await page.waitForFunction(()=>!sizeBusy);assert.equal(await page.evaluate(()=>fixtureSettings.uses_custom_scale),true);assert.equal(await page.$$eval('#seg-size [aria-pressed="true"]',els=>els.length),0);await page.evaluate(()=>{failScale=false;});
  });
  await page.addScriptTag({path:path.join(ui,'matra-providers.js')});
  await page.evaluate(async()=>{
   window.sourceCalls=[];window.failSave=false;window.endpoints=[{id:'one',name:'Fixture one',base_url:'https://one.example.test',icon_preset:'openai',accent_color_hex:'#6366F1',has_key:false},{id:'two',name:'Fixture two',base_url:'https://two.example.test',icon_preset:'openai',accent_color_hex:'#6366F1',has_key:false}];
   window.sourceView=MatraProviders.mount(document.querySelector('#sources'),{invoke:async(name,args)=>{
    sourceCalls.push({name,args});if(name==='list_custom_endpoints')return endpoints;if(name==='get_parity_usage'||name==='get_opencode_bridge_status')return{};
    if(name==='save_custom_endpoint'){if(failSave)throw Error('Synthetic endpoint save refusal');const value={...args.endpoint,has_key:args.secret!==null?args.secret!=='':endpoints.find(e=>e.id===args.endpoint.id)?.has_key};endpoints=endpoints.map(e=>e.id===value.id?value:e);return value;}return[];
   }});await sourceView.load();
  });
  const editor=()=>page.click('[data-endpoint="one"] [data-source-action="edit"]');
  const enter=async(key,remove=false)=>{if(key)await page.type('#source-editor [name="secret"]',key);if(remove)await page.click('#source-editor [name="remove_secret"]');};
  const upload=async(file=iconPath)=>{await(await page.$('#endpoint-icon-file')).uploadFile(file);await page.waitForFunction(()=>!sourceView.state.busy);};
  const safeState=async(key,remove)=>{
   const result=await page.evaluate(k=>{const form=document.querySelector('#source-editor');return{value:form.elements.secret.value,attribute:form.elements.secret.getAttribute('value'),remove:form.elements.remove_secret.checked,markupLeak:document.querySelector('#sources').innerHTML.includes(k),stateLeak:JSON.stringify(sourceView.state).includes(k)};},key);
   assert.equal(result.value,key);assert.equal(result.attribute,'');assert.equal(result.remove,remove);assert.equal(result.markupLeak,false);assert.equal(result.stateLeak,false);
  };
  const save=async(expected)=>{
   const before=await page.evaluate(()=>sourceCalls.filter(c=>c.name==='save_custom_endpoint').length);await page.click('#source-editor button[type="submit"]');await page.waitForFunction(n=>sourceCalls.filter(c=>c.name==='save_custom_endpoint').length>n&&!sourceView.state.busy,{},before);
   const args=await page.evaluate(()=>sourceCalls.filter(c=>c.name==='save_custom_endpoint').at(-1).args);assert.equal(args.secret,expected);assert.equal(Object.hasOwn(args.endpoint,'secret'),false);assert.equal(Object.hasOwn(args.endpoint,'remove_secret'),false);if(expected)assert.equal(JSON.stringify(args.endpoint).includes(expected),false);
  };
  await check('typed key survives icon upload and is submitted only through the separate secret argument',async()=>{await editor();await enter('SYNTHETIC-UPLOAD-KEY');await upload();await page.waitForFunction(()=>sourceView.state.editor.custom_icon_data_url?.startsWith('data:image/png;base64,'));await safeState('SYNTHETIC-UPLOAD-KEY',false);await save('SYNTHETIC-UPLOAD-KEY');});
  await check('typed replacement key survives Use preset icon and never enters markup or endpoint settings',async()=>{await editor();assert.equal(await page.$eval('[name="secret"]',e=>e.value),'');await enter('SYNTHETIC-RESET-KEY');await page.click('[data-source-action="clear-icon"]');await safeState('SYNTHETIC-RESET-KEY',false);await save('SYNTHETIC-RESET-KEY');});
  await check('explicit key removal survives icon upload and wins over a typed value',async()=>{await editor();await enter('SYNTHETIC-REMOVE-UPLOAD',true);await upload();await safeState('SYNTHETIC-REMOVE-UPLOAD',true);await save('');});
  await check('explicit key removal survives Use preset icon',async()=>{await editor();await enter('SYNTHETIC-REMOVE-RESET',true);await page.click('[data-source-action="clear-icon"]');await safeState('SYNTHETIC-REMOVE-RESET',true);await save('');});
  await check('decode and save failures retain transient credentials for an explicit retry',async()=>{
   await editor();await enter('SYNTHETIC-RETRY-KEY');await upload(badIconPath);await page.waitForFunction(()=>sourceView.state.error.length>0);await safeState('SYNTHETIC-RETRY-KEY',false);
   await page.evaluate(()=>{failSave=true;});await save('SYNTHETIC-RETRY-KEY');await safeState('SYNTHETIC-RETRY-KEY',false);await page.evaluate(()=>{failSave=false;});await save('SYNTHETIC-RETRY-KEY');
  });
  await check('successful save, cancel and switching endpoint clear transient key and removal choice',async()=>{
   await editor();assert.equal(await page.$eval('[name="secret"]',e=>e.value),'');assert.equal(await page.$eval('[name="remove_secret"]',e=>e.checked),false);
   await enter('SYNTHETIC-CANCEL-KEY',true);await page.click('[data-source-action="cancel"]');await page.click('[data-source-action="new"]');assert.equal(await page.$eval('[name="secret"]',e=>e.value),'');assert.equal(await page.$eval('[name="remove_secret"]',e=>e.checked),false);
   await enter('SYNTHETIC-SWITCH-KEY',true);await page.click('[data-endpoint="two"] [data-source-action="edit"]');assert.equal(await page.$eval('[name="secret"]',e=>e.value),'');assert.equal(await page.$eval('[name="remove_secret"]',e=>e.checked),false);
   assert.equal(await page.evaluate(()=>JSON.stringify(sourceView.state).includes('SYNTHETIC-')),false);
  });
  assert.deepEqual(errors,[]);assert.deepEqual(network,[]);fs.writeFileSync(path.join(out,'result.json'),JSON.stringify({checks,errors,network},null,2));console.log(JSON.stringify({passed:checks.length,checks}));
 }catch(e){fs.writeFileSync(path.join(out,'failure.json'),JSON.stringify({error:String(e),stack:e.stack,checks,errors,network},null,2));throw e;}finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
