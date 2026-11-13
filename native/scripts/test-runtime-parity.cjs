/* Headless shipped-module replay. All bridge values are labelled fixtures; no provider/OS submission. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {pathToFileURL} = require('node:url');
const {execFileSync} = require('node:child_process');
const puppeteer = require(process.env.MATRA_PUPPETEER_PATH || 'puppeteer');
const repo = path.resolve(__dirname,'../..'), ui = path.join(repo,'native/matra/ui');
const out = path.join(repo,'.runtime-build/browser'); fs.mkdirSync(out,{recursive:true});
assert.ok(fs.realpathSync(out).startsWith(fs.realpathSync(repo)+path.sep));
const review = path.join(out,'notch');
execFileSync(process.execPath,[path.join(repo,'scripts/build-notch-review.mjs'),review],{env:{...process.env,TMPDIR:path.join(repo,'.runtime-build/tmp')}});
const checks=[],errors=[],network=[];
const alertSource=fs.readFileSync(path.join(repo,'native/matra/src/alerts.rs'),'utf8');
assert.match(alertSource,/pub async fn send_test_alert\(app: AppHandle\)/);
assert.match(alertSource,/spawn_blocking\(move \|\| send_test_alert_blocking\(app\)\)/);
checks.push('native test-delivery command explicitly keeps acknowledgement wait off UI and async executor threads');
const check=(name,fn)=>Promise.resolve().then(fn).then(()=>checks.push(name));
(async()=>{
  const browser=await puppeteer.launch({executablePath:process.env.MATRA_CHROME_PATH,headless:true,userDataDir:path.join(out,'profile'),
    env:{...process.env,TMPDIR:path.join(repo,'.runtime-build/tmp'),XDG_CACHE_HOME:path.join(out,'cache')},
    args:['--disable-background-networking','--disable-component-update','--no-first-run','--no-default-browser-check','--disable-crash-reporter','--disable-breakpad','--disable-sync','--disable-features=MediaRouter']});
  try {
    const page=await browser.newPage(); page.setDefaultTimeout(10000); page.on('pageerror',e=>errors.push(e.message));
    await page.setRequestInterception(true); page.on('request',r=>{if(/^https?:/.test(r.url())){network.push(r.url());r.abort();}else r.continue();});
    await page.setViewport({width:920,height:900});
    await page.setContent('<main id="history"></main><main id="alerts"></main><main id="sources"></main>');
    for(const name of ['matra-parity-card.js','matra-alerts.js','matra-providers.js','matra-alert-peek.js'])await page.addScriptTag({path:path.join(ui,name)});
    await check('real history counts, missing inputs, date/model grouping and escaped text',async()=>{
      const value=await page.evaluate(()=>{
        const at=Date.now(),day=new Date(at).toLocaleDateString('en-CA');
        const p={model:'Fixture <img onerror="bad()">',at,input_tokens:null,output_tokens:120,tokens_per_second:30,generation_seconds:4};
        const totals={requests:1,input_tokens:null,output_tokens:120,input_measured:0,output_measured:1};
        const history={state:'ready',message:'SYNTHETIC RESPONSE COUNTS',today:totals,models:[{model:p.model,days:[{day,totals}],last:p}],recent:[p]};
        document.querySelector('#history').innerHTML=MatraParityCard.historyHtml(history,undefined,true,'day',at);
        return {text:document.querySelector('#history').textContent,images:document.querySelector('#history img')!==null,
          unknown:MatraParityCard.historyHtml({state:'unavailable',message:'Unavailable',days:[],models:[]})};
      });
      assert.match(value.text,/120 output/);assert.match(value.text,/input unknown/);assert.match(value.text,/30.0 tok\/s/);assert.match(value.text,/Observed tokens by local day/);
      assert.equal(value.images,false);assert.doesNotMatch(value.unknown,/0 (input|output|tokens|responses)/);
    });
    await check('history Week and Month use the dashboard calendar boundaries',async()=>{
      const result=await page.evaluate(()=>{
        const h={models:[{model:'Calendar fixture',days:['2026-10-31','2026-11-01','2026-11-08','2026-11-09'].map(day=>({day,totals:{requests:1,output_tokens:1,output_measured:1}}))}]};
        return {week:MatraParityCard.historyHtml(h,undefined,true,'week',new Date(2026,10,13,12).getTime()),month:MatraParityCard.historyHtml(h,undefined,true,'month',new Date(2026,10,13,12).getTime())};
      });
      assert.match(result.week,/2026-11-09/);assert.doesNotMatch(result.week,/2026-11-08/);
      assert.match(result.month,/2026-11-01/);assert.doesNotMatch(result.month,/2026-10-31/);
    });
    await page.evaluate(async()=>{
      window.fixtureCalls=[];window.fixtureListeners={};window.failSave=false;
      window.fixtureSettings=MatraAlerts.normalize({enabled:true,channel:'notch',peek:true,peek_duration:'long',work_complete:true,attention:true,sound:true,muted_providers:['claude@sample']});
      window.fixtureStatus={can_deliver:true,message:'SYNTHETIC CHANNEL',channel:'notch',sound_note:'Fixture tones only',providers:[]};
      window.fixtureInvoke=async(command,args)=>{
        fixtureCalls.push({command,args});
        if(command==='get_alert_settings')return fixtureSettings;
        if(command==='get_alert_status')return {...fixtureStatus,channel:fixtureSettings.channel};
        if(command==='set_alert_settings'){if(failSave)throw 'Fixture save refused';fixtureSettings=args.settings;return fixtureSettings;}
        if(command==='send_test_alert')return {...fixtureStatus,channel:fixtureSettings.channel};
        return true;
      };
      window.alertView=MatraAlerts.mount(document.querySelector('#alerts'),{invoke:fixtureInvoke,listen:async(name,fn)=>{fixtureListeners[name]=fn;return()=>{};}});
      await alertView.ready;
    });
    await check('peek duration and independent per-event sound persist complete preferences',async()=>{
      await page.select('#alerts select[aria-label="Peek duration"]','brief');
      await page.waitForFunction(()=>fixtureSettings.peek_duration==='brief');
      await page.select('#alerts select[aria-label="Work finishes: sound"]','chime');
      await page.waitForFunction(()=>fixtureSettings.event_sounds.work_complete==='chime');
      const settings=await page.evaluate(()=>fixtureSettings);
      assert.equal(settings.event_sounds.attention,'default');assert.deepEqual(settings.muted_providers,['claude@sample']);
    });
    await check('system channel hides notch duration and reports system submission wording',async()=>{
      await page.select('#alerts select[aria-label="Notification channel"]','system');
      await page.waitForFunction(()=>fixtureSettings.channel==='system'&&!document.querySelector('#alerts select[aria-label="Peek duration"]'));
      await page.evaluate(()=>[...document.querySelectorAll('#alerts button')].find(b=>b.textContent==='Send a test').click());
      await page.waitForFunction(()=>document.querySelector('#alerts').textContent.includes('The system accepted the test.'));
      assert.doesNotMatch(await page.$eval('#alerts',e=>e.textContent),/Windows accepted/);
    });
    await check('failed preference save rolls back without losing muted accounts',async()=>{
      await page.evaluate(()=>{failSave=true;});
      await page.select('#alerts select[aria-label="Notification channel"]','notch');
      await page.waitForFunction(()=>document.querySelector('#alerts').textContent.includes('previous choice is kept'));
      assert.equal(await page.$eval('#alerts select[aria-label="Notification channel"]',e=>e.value),'system');
      await page.evaluate(()=>{failSave=false;});
    });
    await check('unavailable channel disables delivery and reports the actual reason',async()=>{
      await page.evaluate(()=>fixtureListeners.alert_status({payload:{can_deliver:false,message:'Notch is hidden',providers:[]}}));
      assert.equal(await page.evaluate(()=>[...document.querySelectorAll('#alerts button')].find(b=>b.textContent==='Send a test').disabled),true);
      assert.match(await page.$eval('#alerts',e=>e.textContent),/Notch is hidden/);
    });
    await page.evaluate(async()=>{
      window.historyOptions={lmstudio:{enabled:true,log_directory:null},omlx:{enabled:true,log_directory:null}};
      window.relayStatus={enabled:false,ready:false,address:'http://127.0.0.1:11438',message:'SYNTHETIC RELAY OFF',active:0,observed_responses:0,observation_gaps:0};
      window.sourceCalls=[];
      const invoke=async(command,args)=>{
        sourceCalls.push({command,args});
        if(command==='get_provider_registry')return ['ollama','lmstudio','omlx'].map(id=>({id,name:id,enabled:true,endpoint:'http://127.0.0.1:1234',endpoint_scope:'loopback'}));
        if(command==='get_local_history_settings')return historyOptions;
        if(command==='get_ollama_relay_status')return relayStatus;
        if(command==='get_parity_usage')return {};
        if(command==='get_opencode_bridge_status')return {installed:false};
        if(command==='set_local_history_settings'){historyOptions[args.id]=args.options;return historyOptions;}
        if(command==='set_ollama_relay'){relayStatus={...relayStatus,enabled:args.enabled,ready:args.enabled,message:'SYNTHETIC RELAY READY'};return relayStatus;}
        return [];
      };
      window.sourcesView=MatraProviders.mount(document.querySelector('#sources'),{invoke});await sourcesView.load();
    });
    await check('local log directory and enable controls reach the real command contract',async()=>{
      await page.type('[data-source="lmstudio"] [name="history-directory"]','/fixture/lmstudio/server-logs');
      await page.click('[data-source="lmstudio"] [data-source-action="history-directory"]');
      await page.waitForFunction(()=>historyOptions.lmstudio.log_directory==='/fixture/lmstudio/server-logs');
      await page.click('[data-history-toggle="omlx"]');await page.waitForFunction(()=>historyOptions.omlx.enabled===false);
      const call=await page.evaluate(()=>sourceCalls.find(c=>c.command==='set_local_history_settings'&&c.args.id==='lmstudio'));
      assert.equal(call.args.options.enabled,true);
    });
    await check('explicit Ollama relay control and development address are wired',async()=>{
      await page.click('[data-source-action="ollama-relay"]');await page.waitForFunction(()=>relayStatus.ready);
      assert.equal(await page.$eval('[aria-label="Ollama relay client base URL"]',e=>e.value),'http://127.0.0.1:11438');
      assert.match(await page.$eval('[data-source="ollama"]',e=>e.textContent),/Stop relay/);
    });
    await page.screenshot({path:path.join(out,'runtime-controls.png'),fullPage:true});

    await check('notch acknowledgement is exactly once across replacement and disposal',async()=>{
      const result=await page.evaluate(async()=>{
        const acks=[];let release;const play=()=>new Promise(r=>{release=r;});
        const view=MatraAlertPeek.mount({listen:async()=>()=>{},invoke:async(_,args)=>acks.push(args),anchor:()=>({left:20,top:20,right:60,bottom:100,width:40,height:80}),edge:()=> 'left',canShow:()=>true,onChange:()=>{},play});
        const first=view.receive({payload:{id:'old',toast:{title:'A',body:'B',peek:true,silent:false,sound:'chime',duration_ms:5000}}});
        await view.receive({payload:{id:'new',toast:{title:'C',body:'D',peek:true,silent:true,duration_ms:5000}}});
        release();await first;view.dispose();
        return acks;
      });
      assert.equal(result.filter(a=>a.id==='old').length,1);assert.match(result.find(a=>a.id==='old').error,/replaced/);
      assert.equal(result.filter(a=>a.id==='new').length,1);assert.equal(result.find(a=>a.id==='new').error,null);
    });
    await check('sound failure is acknowledged as failure instead of claimed delivery',async()=>{
      const result=await page.evaluate(async()=>{
        const acks=[];const view=MatraAlertPeek.mount({listen:async()=>()=>{},invoke:async(_,a)=>acks.push(a),anchor:()=>({left:20,top:20,right:60,bottom:100,width:40,height:80}),edge:()=> 'left',canShow:()=>true,onChange:()=>{},play:async()=>{throw new Error('Fixture audio blocked');}});
        await view.receive({payload:{id:'audio',toast:{peek:false,silent:false,sound:'soft'}}});view.dispose();return acks;
      });assert.equal(result.length,1);assert.match(result[0].error,/audio blocked/);
    });
    await check('closing during pending audio acknowledges failure exactly once',async()=>{
      const result=await page.evaluate(async()=>{
        const acks=[];let release;const view=MatraAlertPeek.mount({listen:async()=>()=>{},invoke:async(_,a)=>acks.push(a),anchor:()=>({left:20,top:20,right:60,bottom:100,width:40,height:80}),edge:()=> 'left',canShow:()=>true,onChange:()=>{},play:()=>new Promise(r=>{release=r;})});
        const pending=view.receive({payload:{id:'closing',toast:{peek:true,silent:false,sound:'soft'}}});view.dispose();release();await pending;return acks;
      });assert.equal(result.length,1);assert.match(result[0].error,/closed/);
    });
    for(const edge of ['left','right','top','bottom'])for(const scale of [0.8,1.25]){
      await page.setViewport({width:['left','right'].includes(edge)?460:650,height:650,deviceScaleFactor:1});
      await page.goto(pathToFileURL(path.join(review,'windows-notch-component.html')).href+'?scale='+scale);
      await page.waitForFunction(()=>window.matraAlertPeek&&window.__matraSample);
      await page.evaluate(({edge,scale})=>{__matraSample.configure({edge,count:3,fold:true,size:scale});}, {edge,scale});
      await check(`shipped notch paints, acknowledges and keeps alert inside viewport: ${edge}/${scale}`,async()=>{
        await page.evaluate(()=>__matraSample.emit('notch_alert',{id:'fixture-alert',toast:{title:'Fixture response completed',body:'Count-only synthetic session',peek:true,silent:true,duration_ms:3000}}));
        await page.waitForFunction(()=>__matraSample.calls.some(c=>c.name==='acknowledge_notch_alert'&&c.args.id==='fixture-alert'));
        const result=await page.evaluate(()=>{const panel=document.querySelector('.matra-alert-peek'),r=panel.getBoundingClientRect();return {hidden:panel.hidden,text:panel.textContent,left:r.left,top:r.top,right:r.right,bottom:r.bottom,w:innerWidth,h:innerHeight,ack:__matraSample.calls.filter(c=>c.name==='acknowledge_notch_alert').at(-1).args};});
        assert.equal(result.hidden,false);assert.equal(result.ack.error,null);assert.match(result.text,/Fixture response completed/);
        assert.ok(result.left>=-1&&result.top>=-1&&result.right<=result.w+1&&result.bottom<=result.h+1,JSON.stringify(result));
      });
    }
    await page.screenshot({path:path.join(out,'notch-alert.png')});
    await check('shipped notch refuses alerts during carry and cleans up the visible peek',async()=>{
      await page.evaluate(()=>{carrying=true;document.body.classList.add('carrying');});
      await page.waitForFunction(()=>document.querySelector('.matra-alert-peek').hidden);
      await page.evaluate(()=>__matraSample.emit('notch_alert',{id:'busy',toast:{title:'Busy',peek:true,silent:true}}));
      await page.waitForFunction(()=>__matraSample.calls.some(c=>c.name==='acknowledge_notch_alert'&&c.args.id==='busy'));
      const ack=await page.evaluate(()=>__matraSample.calls.find(c=>c.name==='acknowledge_notch_alert'&&c.args.id==='busy').args);
      assert.match(ack.error,/move|hidden/);
    });
    await check('three-second peek expires and removes its interactive region',async()=>{
      await page.evaluate(()=>{carrying=false;document.body.classList.remove('carrying');__matraSample.emit('notch_alert',{id:'expiry',toast:{title:'Expiry fixture',peek:true,silent:true,duration_ms:3000}});});
      await page.waitForFunction(()=>!document.querySelector('.matra-alert-peek').hidden);
      await page.waitForFunction(()=>document.querySelector('.matra-alert-peek').hidden,{timeout:5000});
      assert.equal(await page.evaluate(()=>window.matraAlertPeek.rect()),null);
      assert.equal(await page.evaluate(()=>__matraSample.calls.filter(c=>c.name==='acknowledge_notch_alert'&&c.args.id==='expiry').length),1);
    });
    assert.deepEqual(errors,[]);assert.deepEqual(network,[]);
  } finally { await browser.close();fs.writeFileSync(path.join(out,'report.json'),JSON.stringify({checks,errors,network},null,2)); }
  process.stdout.write(`${checks.length} runtime browser checks passed; no page errors or network requests.\n`);
})().catch(error=>{process.stderr.write(String(error.stack||error)+'\n');process.exitCode=1;});
