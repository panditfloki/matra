// Production controls and notch renderer, synthetic IPC only. No real account or OS submission.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{pathToFileURL}=require('node:url'),{execFileSync}=require('node:child_process');
const puppeteer=require(process.env.MATRA_PUPPETEER_PATH||'puppeteer');
const repo=path.resolve(__dirname,'../..'),ui=path.join(repo,'native/matra/ui'),out=path.join(repo,'.presentation-validation/browser');
for(const n of ['','tmp','cache','profile'])fs.mkdirSync(path.join(out,n),{recursive:true});
assert.ok(fs.realpathSync(out).startsWith(fs.realpathSync(repo)+path.sep));
const review=path.join(out,'notch');execFileSync(process.execPath,[path.join(repo,'scripts/build-notch-review.mjs'),review],{env:{...process.env,TMPDIR:path.join(out,'tmp')}});
// Extend only the inert fixture values to exercise arbitrary supported scales and fullscreen states.
const component=path.join(review,'windows-notch-component.html');let html=fs.readFileSync(component,'utf8');
html=html.replace('[.8, 1, 1.25].includes(requestedScale)','(Number.isFinite(requestedScale)&&requestedScale>=.5&&requestedScale<=1.5)').replace('[.8, 1, 1.25].includes(size)','(Number.isFinite(size)&&size>=.5&&size<=1.5)').replace("if (Object.hasOwn(values, name))", "if(name==='get_fullscreen_state')return window.fixtureFullscreen===true;\n      if (Object.hasOwn(values, name))");fs.writeFileSync(component,html);
const checks=[],errors=[],network=[];const pause=ms=>new Promise(r=>setTimeout(r,ms));const check=async(name,fn)=>{await fn();checks.push(name);};
(async()=>{
 const browser=await puppeteer.launch({executablePath:process.env.MATRA_CHROME_PATH,headless:true,userDataDir:path.join(out,'profile'),env:{...process.env,TMPDIR:path.join(out,'tmp'),XDG_CACHE_HOME:path.join(out,'cache')},args:['--disable-background-networking','--disable-component-update','--no-first-run','--no-default-browser-check','--disable-crash-reporter','--disable-breakpad','--disable-sync','--disable-features=MediaRouter']});
 try{
  const page=await browser.newPage();page.setDefaultTimeout(10000);page.on('pageerror',e=>errors.push(e.message));await page.setRequestInterception(true);page.on('request',r=>{if(/^https?:/.test(r.url())){network.push(r.url());r.abort();}else r.continue();});
  await page.setViewport({width:900,height:1200});
  // Use the actual Display pane and its shipped controller, with an inert settings store.
  const settings=fs.readFileSync(path.join(ui,'settings.html'),'utf8');
  await page.setContent('<style>.switch{min-width:36px;min-height:22px}#account-presentation{margin:12px}button,input{margin:5px}</style><main id="controls"></main><main id="sources"></main>');
  await page.evaluate(html=>{
   const doc=new DOMParser().parseFromString(html,'text/html'),host=document.querySelector('#controls');
   for(const id of ['sw-weekly-headline','sw-weekly-reading','sw-weekly-dashed','sw-daily-pace','sw-usage-pace','sw-extra-limits','sw-refresh-look','sw-fullscreen','sw-custom-scale','custom-scale-row','account-presentation','parity-status','fullscreen-cap'])host.append(doc.getElementById(id).cloneNode(true));
  },settings);
  for(const n of ['matra-parity.js','matra-accounts.js'])await page.addScriptTag({path:path.join(ui,n)});
  await page.evaluate(()=>{
   window.fixtureSettings=MatraParity.normalize({account_order:['claude@away','claude@work','claude'],account_nicknames:{'claude@away':'Archived'}});window.fixtureCalls=[];window.listeners={};window.failSave=false;
   window.__TAURI__={core:{invoke:async(name,args)=>{fixtureCalls.push({name,args});if(name==='get_parity_settings')return fixtureSettings;if(name==='set_parity_settings'){if(failSave)throw Error('Fixture refusal');fixtureSettings=args.settings;return fixtureSettings;}if(name==='get_usage')return {status:'ok',accounts:[{id:'claude',display_name:'default',windows:[]},{id:'claude@work',display_name:'Office',windows:[]}]};if(name==='get_platform_info')return{capabilities:{fullscreen_detection:true}};return null;}},event:{listen:async(name,fn)=>{listeners[name]=fn;return()=>{};}}};
  });
  await page.addScriptTag({path:path.join(ui,'matra-parity-settings.js')});await page.waitForFunction(()=>!document.querySelector('#sw-weekly-headline').disabled);
  await check('persisted production switches preserve unrelated account preferences and rollback errors',async()=>{
   for(const [id,key]of [['sw-weekly-headline','weekly_headline'],['sw-weekly-reading','weekly_reading'],['sw-weekly-dashed','weekly_ring_dashed'],['sw-daily-pace','claude_daily_pace'],['sw-usage-pace','show_usage_pace'],['sw-refresh-look','asks_provider_on_look']]){await page.click('#'+id);assert.equal(await page.evaluate(k=>fixtureSettings[k],key),true);}
   assert.deepEqual(await page.evaluate(()=>fixtureSettings.account_order),['claude@away','claude@work','claude']);
   await page.evaluate(()=>{failSave=true;});await page.click('#sw-weekly-headline');await pause(20);assert.equal(await page.$eval('#sw-weekly-headline',e=>e.getAttribute('aria-checked')),'true');await page.evaluate(()=>{failSave=false;});
  });
  await check('custom scale and stable account names/order reach the production save contract',async()=>{
   await page.click('#sw-custom-scale');await page.$eval('#custom-scale',e=>{e.value='1.37';e.dispatchEvent(new Event('change',{bubbles:true}));});await page.waitForFunction(()=>fixtureSettings.custom_scale===1.37);assert.equal(await page.evaluate(()=>fixtureSettings.uses_custom_scale),true);
   await page.type('input[aria-label="Office nickname"]','Work');await page.$eval('input[aria-label="Office nickname"]',e=>e.dispatchEvent(new Event('change',{bubbles:true})));await page.waitForFunction(()=>fixtureSettings.account_nicknames['claude@work']==='Work');await page.click('#sw-usage-pace');await page.click('button[aria-label="Move Office up"]');await page.waitForFunction(()=>fixtureSettings.account_order[0]==='claude@work');assert.equal(await page.evaluate(()=>fixtureSettings.account_nicknames['claude@away']),'Archived');
  });
  await page.addScriptTag({path:path.join(ui,'matra-providers.js')});
  await page.evaluate(async()=>{window.sourceCalls=[];window.endpoint={id:'fixture',name:'Fixture',base_url:'https://example.test',icon_preset:'openai',accent_color_hex:'#6366F1'};window.sourceView=MatraProviders.mount(document.querySelector('#sources'),{invoke:async(name,args)=>{sourceCalls.push({name,args});if(name==='list_custom_endpoints')return[endpoint];if(name==='get_parity_usage')return{};if(name==='get_opencode_bridge_status')return{};if(name==='save_custom_endpoint'){endpoint=args.endpoint;return endpoint;}return[];}});await sourceView.load();});
  await check('custom endpoint preset, colour and uploaded PNG persist through real editor controls',async()=>{
   await page.click('[data-source-action="edit"]');await page.select('[name="icon_preset"]','meta');await page.$eval('[name="accent_color_hex"]',e=>e.value='#12ab34');
   const icon=require('../matra/fixtures/presentation.json').icon;const iconPath=path.join(out,'fixture-icon.png');fs.writeFileSync(iconPath,Buffer.from(icon.split(',')[1],'base64'));
   const upload=await page.$('#endpoint-icon-file');await upload.uploadFile(iconPath);await page.waitForFunction(()=>sourceView.state.editor.custom_icon_data_url?.startsWith('data:image/png;base64,'));await page.click('#source-editor button[type="submit"]');await page.waitForFunction(()=>sourceCalls.some(c=>c.name==='save_custom_endpoint'));const e=await page.evaluate(()=>endpoint);assert.equal(e.icon_preset,'meta');assert.equal(e.accent_color_hex,'#12ab34');assert.match(e.custom_icon_data_url,/^data:image\/png;base64,/);
  });
  await page.goto(pathToFileURL(component).href);await page.waitForFunction(()=>window.__matraSample&&document.querySelector('.cell'));
  await check('shipped notch draws weekly headline, dashed secondary, pair and daily pace card',async()=>{
   await page.evaluate(()=>{__matraSample.configure({edge:'left',count:3,fold:false});__matraSample.emit('parity_settings',{weekly_headline:true,weekly_ring_dashed:true,weekly_reading:true,show_usage_pace:true});});
   assert.match(await page.$eval('.cell[data-p="claude"] .pct',e=>e.textContent),/\//);assert.equal(await page.$eval('.cell[data-p="claude"] .quota-weekly',e=>e.tagName.toLowerCase()),'path');
   await page.evaluate(()=>{__matraSample.emit('parity_settings',{claude_daily_pace:true,show_usage_pace:true});hoverId='claude';renderCard();});assert.match(await page.$eval('#card',e=>e.textContent),/Daily pace/);assert.match(await page.$eval('#card',e=>e.textContent),/reserved|deficit/);
  });
  await check('refresh on look is opt-in and fullscreen folding yields to pointer and carry',async()=>{
   await page.evaluate(()=>{lastLook=0;__matraSample.calls.length=0;__matraSample.emit('parity_settings',{asks_provider_on_look:false});refreshOnLook();});assert.equal(await page.evaluate(()=>__matraSample.calls.some(c=>c.name==='refresh_on_look')),false);
   await page.evaluate(()=>{__matraSample.emit('parity_settings',{asks_provider_on_look:true});refreshOnLook();});assert.equal(await page.evaluate(()=>__matraSample.calls.some(c=>c.name==='refresh_on_look')),true);
   await page.evaluate(async()=>{pointerIn=false;peekUntil=0;onHover=false;fixtureFullscreen=true;await checkFullscreen();});await page.waitForFunction(()=>folded);await page.evaluate(()=>__matraSample.emit('notch_pointer',true));assert.equal(await page.evaluate(()=>folded),false);
   await page.evaluate(()=>{pointerIn=false;carrying=true;scheduleFold();});await pause(500);assert.equal(await page.evaluate(()=>folded),false);await page.evaluate(async()=>{carrying=false;fixtureFullscreen=false;await checkFullscreen();});assert.equal(await page.evaluate(()=>folded),false);
  });
  await check('Unicode and quoted account IDs retain exact nickname, ring and card targeting',async()=>{
   const result=await page.evaluate(()=>{const id='claude@工作/"office"';__matraSample.emit('usage',{status:'ok',accounts:[{id,display_name:'Fixture',status:'ok',fetched_at:Date.now(),windows:[{id:'session',label:'Session',used:.2}]}],windows:[]});__matraSample.emit('parity_settings',{account_nicknames:{[id]:'कार्य'}});hoverId=id;renderCard();placeCard();const cell=[...pill.querySelectorAll('.cell')].find(c=>c.dataset.p===id);return {reading:cell?.querySelector('.pct').textContent,title:document.querySelector('#card .c-title').textContent};});assert.equal(result.reading,'20%');assert.match(result.title,/कार्य/);
  });
  for(const scale of [.5,.8,1,1.25,1.37,1.5])for(const edge of ['left','top'])await check(`custom/preset size retains measured geometry and carry scale ${edge}/${scale}`,async()=>{
   await page.setViewport({width:edge==='left'?460:650,height:650});await page.goto(pathToFileURL(component).href+'?scale='+scale);await page.waitForFunction(()=>window.__matraSample&&document.querySelector('.cell'));await page.evaluate(({edge,scale})=>{__matraSample.configure({edge,count:10,size:scale});reportDpr();},{edge,scale});await page.waitForFunction(s=>notchScale===s,{},scale);await pause(80);
   const g=await page.evaluate(()=>{const r=pill.getBoundingClientRect();return{w:innerWidth,h:innerHeight,x:r.x,y:r.y,right:r.right,bottom:r.bottom,scale:notchScale,cardScale:getComputedStyle(document.documentElement).getPropertyValue('--notch-card-scale')};});assert.ok(g.x>=-1&&g.y>=-1&&g.right<=g.w+1&&g.bottom<=g.h+1,JSON.stringify(g));assert.equal(g.scale,scale);assert.ok(Math.abs(Number(g.cardScale||1)-1/scale)<1e-10);
  });
  assert.deepEqual(errors,[]);assert.deepEqual(network,[]);fs.writeFileSync(path.join(out,'result.json'),JSON.stringify({checks,errors,network},null,2));console.log(JSON.stringify({passed:checks.length,checks}));
 }catch(e){fs.writeFileSync(path.join(out,'failure.json'),JSON.stringify({error:String(e),stack:e.stack,checks,errors,network},null,2));throw e;}finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
