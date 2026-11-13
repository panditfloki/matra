// Shipped notch + real carry renderer, inert browser/native boundary. Does not
// certify HWND/X11 capture, mixed physical monitors, taskbars or native materials.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const{execFileSync}=require('node:child_process'),{pathToFileURL}=require('node:url');
const puppeteer=require(process.env.MATRA_PUPPETEER_PATH||'puppeteer');
const repo=path.resolve(__dirname,'../..'),out=path.resolve(process.env.MATRA_CARRY_EVIDENCE||path.join(repo,'artifacts/carry-replay'));
assert.ok(out.startsWith('/Users/pandit/KREMLIN/'),'Replay evidence must remain contained');
fs.mkdirSync(out,{recursive:true});assert.ok(fs.realpathSync(out).startsWith('/Users/pandit/KREMLIN/'));
for(const name of ['profile','tmp','cache'])fs.mkdirSync(path.join(out,name),{recursive:true});
const component=path.join(repo,'artifacts/carry-component');
execFileSync(process.execPath,[path.join(repo,'scripts/build-notch-review.mjs'),component],{stdio:'pipe'});
const rustc=process.env.MATRA_RUSTC||'/Users/pandit/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin/rustc';
execFileSync(rustc,['--edition=2021','-A','dead_code',path.join(__dirname,'carry-replay.rs'),'-o',path.join(out,'replay-engine')],{stdio:'pipe',env:{...process.env,TMPDIR:path.join(out,'tmp')}});
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
// Inspect real screenshot pixels. A decoded SVG and a populated <image> can still
// render blank (for example, an invalid clipPath child), so DOM assertions alone
// cannot establish that the actual provider rings are visible.
function chromaticPixels(png){
 const zlib=require('node:zlib'),data=Buffer.from(png),chunks=[];let width,height,type,bits;
 for(let at=8;at<data.length;){const length=data.readUInt32BE(at),kind=data.toString('ascii',at+4,at+8),part=data.subarray(at+8,at+8+length);if(kind==='IHDR'){width=part.readUInt32BE(0);height=part.readUInt32BE(4);bits=part[8];type=part[9];}if(kind==='IDAT')chunks.push(part);at+=length+12;}
 assert.equal(bits,8);assert.ok(type===2||type===6);const bpp=type===6?4:3,stride=width*bpp,raw=zlib.inflateSync(Buffer.concat(chunks));let prev=Buffer.alloc(stride),offset=0,count=0;
 const paeth=(a,b,c)=>{const p=a+b-c,pa=Math.abs(p-a),pb=Math.abs(p-b),pc=Math.abs(p-c);return pa<=pb&&pa<=pc?a:pb<=pc?b:c;};
 for(let y=0;y<height;y++){const filter=raw[offset++],row=Buffer.alloc(stride);for(let x=0;x<stride;x++){const a=x>=bpp?row[x-bpp]:0,b=prev[x],c=x>=bpp?prev[x-bpp]:0,adjust=filter===0?0:filter===1?a:filter===2?b:filter===3?Math.floor((a+b)/2):paeth(a,b,c);row[x]=(raw[offset++]+adjust)&255;}
  for(let x=0;x<stride;x+=bpp){const rgb=[row[x],row[x+1],row[x+2]];if(Math.max(...rgb)-Math.min(...rgb)>35&&Math.max(...rgb)>70&&(bpp===3||row[x+3]>100))count++;}prev=row;
 }return count;
}
(async()=>{
 const browser=await puppeteer.launch({executablePath:process.env.MATRA_CHROME_PATH,headless:true,userDataDir:path.join(out,'profile'),
  env:{...process.env,TMPDIR:path.join(out,'tmp'),XDG_CACHE_HOME:path.join(out,'cache')},args:['--disable-background-networking','--disable-component-update','--no-first-run','--no-default-browser-check','--disable-crash-reporter','--disable-breakpad','--disable-sync','--disable-features=MediaRouter']});
 const errors=[],network=[],checks=[];
 try{
  const page=await browser.newPage();page.setDefaultTimeout(12000);page.on('pageerror',e=>errors.push(e.message));
  await page.setRequestInterception(true);page.on('request',r=>{if(/^https?:/.test(r.url())){network.push(r.url());r.abort();}else r.continue();});
  await page.setViewport({width:460,height:650,deviceScaleFactor:1.5});
  await page.goto(pathToFileURL(path.join(component,'windows-notch-component.html')).href);
  await page.waitForFunction(()=>document.querySelectorAll('.cell').length===3);
  await page.evaluate(()=>{window.__matraSample.manualCarry=true;window.__matraSample.configure({edge:'right',count:3});});await pause(120);
  const start=async(alt=false)=>page.evaluate(alt=>{
   if(carryLifecycle.active())throw Error('Previous carry still active');
   clearTimeout(foldTimer);onHover=false;setFolded(false);const r=pill.getBoundingClientRect();
   if(alt){pill.dispatchEvent(new MouseEvent('mousedown',{button:0,altKey:true,clientX:r.x+20,clientY:r.y+30,bubbles:true}));document.dispatchEvent(new MouseEvent('mousemove',{altKey:true,clientX:r.x+20,clientY:r.y+40,bubbles:true}));}
   else{placeHandles();const m=moveHandle.getBoundingClientRect();moveHandle.dispatchEvent(new MouseEvent('mousedown',{button:0,clientX:m.x+m.width/2,clientY:m.y+m.height/2,bubbles:true}));}
   return carryLifecycle.id;
  },alt);
  const finish=async(id,reason=null)=>{await page.evaluate(({id,reason})=>window.__matraSample.emit('carry_end',{id,status:reason?'cancelled':'finished',reason}),{id,reason});await pause(70);assert.equal(await page.evaluate(()=>carryLifecycle.active()||carrying||dragging||document.body.classList.contains('carry-hidden')),false);};
  // Alt-click must never refresh; regular click still does.
  await page.evaluate(()=>{
   const r=pill.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+35;
   pill.dispatchEvent(new MouseEvent('mousedown',{button:0,altKey:true,clientX:x,clientY:y,bubbles:true}));
   document.dispatchEvent(new MouseEvent('mouseup',{button:0,altKey:true,clientX:x,clientY:y,bubbles:true}));
  });
  assert.equal(await page.evaluate(()=>__matraSample.calls.filter(c=>c.name==='refresh_ring').length),0);checks.push('Alt-click suppresses refresh');
  let id=await start();assert.ok(id);
  const presentation=await page.evaluate(()=>__matraSample.calls.findLast(c=>c.name==='begin_move').args.request.presentation);
  assert.equal(presentation.cells.length,3);assert.ok(decodeURIComponent(presentation.cells[0].image).includes('17%'));
  assert.ok(decodeURIComponent(presentation.cells[0].image).includes('data-provider="claude"'));checks.push('Actual provider cells and readings are frozen, with no sample renderer');
  await page.evaluate(id=>{
   __matraSample.emit('carry_present',id);__matraSample.emit('usage',{status:'ok',windows:[{id:'session',used:.81}],fetched_at:Date.now()});
   __matraSample.emit('notch_pointer',true);refreshOnLook();refreshRing('claude');setFolded(true);
  },id);
  assert.equal(await page.evaluate(()=>document.querySelector('.cell .pct').textContent),'17%');
  assert.equal(await page.evaluate(()=>folded),false);assert.equal(await page.evaluate(()=>__matraSample.calls.filter(c=>c.name==='refresh_ring').length),0);checks.push('Carry suppresses provider rerender, hover refresh and fold');
  await page.evaluate(id=>{__matraSample.emit('carry_end',{id:id-1,status:'finished'});document.dispatchEvent(new MouseEvent('mouseup',{button:0,bubbles:true}));},id);
  assert.equal(await page.evaluate(()=>carrying),true);
  assert.ok(await page.evaluate(id=>__matraSample.calls.some(c=>c.name==='carry_pointer_end'&&c.args.id===id&&!c.args.cancel),id));
  await finish(id);assert.equal(await page.evaluate(()=>document.querySelector('.cell .pct').textContent),'81%');
  const beforeAck=await page.evaluate(id=>__matraSample.calls.filter(c=>c.name==='carry_revealed'&&c.args.id===id).length,id);
  await finish(id);assert.equal(await page.evaluate(id=>__matraSample.calls.filter(c=>c.name==='carry_revealed'&&c.args.id===id).length,id),beforeAck);checks.push('Stale tokens ignored; terminal cleanup acknowledged exactly once');
  await page.evaluate(()=>{__matraSample.deferCarry=true;});id=await start(true);
  await page.evaluate(()=>document.dispatchEvent(new MouseEvent('mouseup',{button:0,bubbles:true})));await finish(id);
  await page.evaluate(id=>{__matraSample.resolveCarry(id);__matraSample.deferCarry=false;},id);await pause(30);
  assert.equal(await page.evaluate(()=>carrying||dragging),false);checks.push('Release and terminal event before begin IPC promise resolves');
  id=await start();await page.evaluate(()=>document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})));
  assert.ok(await page.evaluate(id=>__matraSample.calls.some(c=>c.name==='carry_pointer_end'&&c.args.id===id&&c.args.cancel),id));await finish(id,'Carry cancelled');checks.push('Escape cancellation');
  for(const reason of ['Carry cancelled or capture lost','Display geometry changed during carry','Destination display was removed before saving','Carry overlay closed','Notch window closed','Cursor read failed']){
   id=await start();await page.evaluate(id=>__matraSample.emit('carry_present',id),id);await finish(id,reason);checks.push('Terminal recovery: '+reason);
  }
  await page.evaluate(()=>{__matraSample.failCarry='Native pointer capture is unavailable';});id=await start();await pause(70);
  assert.equal(await page.evaluate(()=>carrying||dragging||carryLifecycle.active()),false);await page.evaluate(()=>{__matraSample.failCarry=null;});checks.push('Native capture rejection restores ordinary controls');
  // Measured body anchoring permits a real body within 100pt of a viewport end.
  await page.evaluate(()=>__matraSample.emit('notch_anchor',{edge:'right',along:140}));
  const bodyCenter=await page.evaluate(()=>{const r=pill.getBoundingClientRect();return(r.top+r.bottom)/2;});assert.ok(Math.abs(bodyCenter-140)<.1);checks.push('Body anchor is independent of 650pt viewport centre');
  id=await start();await page.evaluate(id=>{__matraSample.emit('carry_present',id);__matraSample.emit('carry_layout',{id,stage:1});},id);await pause(120);
  const ack=await page.evaluate(id=>__matraSample.calls.findLast(c=>c.name==='carry_layout_ready'&&c.args.id===id),id);
  assert.equal(ack.args.stage,1);assert.ok(Math.abs(ack.args.center[1]/1.5-140)<.2);assert.equal(ack.args.layout.centers.length,3);await finish(id);checks.push('Landing acknowledges actual DPR-scaled body measurement');
  // Feed frames emitted by the actual dependency-free Rust controller to the
  // shipped overlay, including every corner and the final spring landing.
  const layouts=presentation.layouts.map(l=>[l.length,l.depth,l.trailing,l.inset,l.cell_shift,...l.centers].join(','));
  const frames=execFileSync(path.join(out,'replay-engine'),[String(presentation.scale),...layouts],{encoding:'utf8'}).trim().split('\n').map(JSON.parse);
  fs.writeFileSync(path.join(out,'controller-frames.json'),JSON.stringify(frames));
  const overlay=await browser.newPage();overlay.on('pageerror',e=>errors.push(e.message));await overlay.setViewport({width:1200,height:900,deviceScaleFactor:1});
  await overlay.setRequestInterception(true);overlay.on('request',r=>{if(/^https?:/.test(r.url())){network.push(r.url());r.abort();}else r.continue();});
  await overlay.evaluateOnNewDocument(scene=>{
   const listeners=new Map();window.__carryTest={calls:[],emit:(name,payload)=>{for(const h of listeners.get(name)||[])h({payload});}};
   window.__TAURI__={core:{invoke:async(name,args)=>{__carryTest.calls.push({name,args});return name==='get_zones'?scene:null;}},event:{listen:async(name,h)=>{if(!listeners.has(name))listeners.set(name,[]);listeners.get(name).push(h);return()=>{};}}};
  },{presentation,frame:frames[0]});
  await overlay.goto(pathToFileURL(path.join(repo,'native/matra/ui/dropzones.html')).href);
  await overlay.waitForFunction(()=>__carryTest.calls.some(c=>c.name==='carry_overlay_ready'));
  assert.equal(await overlay.$eval('#cells',el=>el.children.length),3);
  const sourceHref=await overlay.$eval('#cells image',el=>el.getAttribute('href'));assert.equal(sourceHref,presentation.cells[0].image);
  const straightPng=await overlay.screenshot({path:path.join(out,'carry-straight.png'),omitBackground:false});
  assert.ok(chromaticPixels(straightPng)>100,'Actual quota-ring pixels must be visible, not only image elements');checks.push('Rendered screenshot contains actual coloured quota-ring pixels');
  for(const corner of ['topRight','bottomRight','bottomLeft','topLeft']){
   const matches=frames.filter(f=>f.corner===corner);assert.ok(matches.length>5,corner+' actually crossed by native controller');
   const frame=matches.reduce((best,f)=>Math.abs(f.before-f.after)<Math.abs(best.before-best.after)?f:best);
   await overlay.evaluate(f=>__carryTest.emit('carry_frame',f),frame);await pause(35);
   const drawn=await overlay.evaluate(()=>({paths:document.querySelectorAll('#body path').length,rings:[...document.querySelectorAll('#cells image')].map(el=>[Number(el.getAttribute('x')),Number(el.getAttribute('y'))]),dots:document.querySelectorAll('#grip circle').length}));
   assert.equal(drawn.paths,2);assert.equal(drawn.dots,6);assert.ok(drawn.rings.flat().every(Number.isFinite));
   const png=await overlay.screenshot({path:path.join(out,'carry-'+corner+'.png'),omitBackground:false});assert.ok(chromaticPixels(png)>50,corner+' keeps the actual coloured ring pixels');checks.push('Actual body and rings cross '+corner);
  }
  await overlay.evaluate(f=>__carryTest.emit('carry_frame',f),frames.at(-1));await pause(35);assert.equal(frames.at(-1).corner,null);
  await overlay.screenshot({path:path.join(out,'carry-settled.png'),omitBackground:false});checks.push('Native spring replay ends in a straight measured landing');
  await overlay.evaluate(p=>__carryTest.emit('carry_presentation',{...p,scale:p.scale*.8}),presentation);await pause(35);
  assert.ok(Math.abs(await overlay.$eval('#cells image',el=>Number(el.getAttribute('width')))-presentation.cells[0].width*presentation.scale*.8)<.01);checks.push('Measured narrow-display landing resizes the real cell images');
  assert.deepEqual(errors,[]);assert.deepEqual(network,[]);
  const result={passed:true,checks,errors,network,frames:frames.length,nativeCapture:false,fixtureBoundary:true};fs.writeFileSync(path.join(out,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
 }catch(error){fs.writeFileSync(path.join(out,'failure.json'),JSON.stringify({error:String(error),stack:error.stack,errors,checks},null,2));throw error;}finally{await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
