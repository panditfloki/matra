// CSS-to-physical scale for native hot rectangles and carry coordinates (matra-native-scale.js).
// Pure logic and its async measurement races; the browser harness covers the shipped notch page.
const {test}=require('node:test');
const assert=require('node:assert/strict');
const S=require('../matra/ui/matra-native-scale.js');
const close=(a,b,m)=>assert.ok(Math.abs(a-b)<1e-9,`${m||''}: ${a} != ${b}`);
const tick=()=>new Promise(resolve=>setImmediate(resolve));

// Linux CI7 (evidence/linux-ci7.log): window 368x520 physical, DPR 1, WebKit zoom .8/.64/.512.
test('WebKitGTK page zoom: physical scale is the window over the viewport, not the constant DPR',()=>{
  for(const [w,h,zoom] of [[460,650,.8],[575,812,.64],[718,1015,.512]]){
    const r=S.resolve({width:368,height:520},[w,h],1);
    assert.equal(r.dprExact,false);
    assert.ok(Math.abs(r.k-zoom)<0.002,`${w}x${h}: ${r.k} vs ${zoom}`);
  }
  // HiDPI GDK scale 2 with the .8 size: DPR 2, physical 2*368.
  close(S.resolve({width:736,height:1040},[460,650],2).k,1.6);
});

test('WebView2 and unzoomed hosts keep exactly devicePixelRatio, including fractional rounding',()=>{
  for(const dpr of [1,1.25,1.5,1.75,2,2.25,3]) for(const [w,h] of [[460,650],[650,460],[459,650],[407,575]]){
    // WebView2 sizes its CSS viewport as the floored/rounded physical/DPR.
    for(const round of [Math.floor,Math.round,Math.ceil]){
      const r=S.resolve({width:round(w*dpr),height:round(h*dpr)},[w,h],dpr);
      assert.ok(r&&r.dprExact,`${dpr} ${w}x${h} ${round.name}`);assert.equal(r.k,dpr);
    }
  }
});

test('a window and page caught in different shapes is not a measurement',()=>{
  assert.equal(S.ratio({width:368,height:520},[460,812]),null);
  assert.equal(S.ratio({width:0,height:520},[460,650]),null);
  assert.equal(S.ratio({width:368,height:520},[0,650]),null);
  assert.equal(S.ratio(null,[460,650]),null);
  assert.equal(S.ratio({width:NaN,height:520},[460,650]),null);
});

function host(initial){
  const h={dpr:initial.dpr,view:[...initial.view],physical:{...initial.physical},calls:0,changes:[],delay:null,pending:[],fail:null,absent:false};
  h.accepts=[];h.fails=[];
  h.scale=S.create({dpr:()=>h.dpr,viewport:()=>h.view,onChange:k=>h.changes.push(k),wait:()=>Promise.resolve(),
    onAccept:(k,changed)=>h.accepts.push({k,changed}),onFail:reason=>h.fails.push(reason),
    measure:initial.absent?null:async()=>{
      h.calls++;
      if(h.fail)throw Error(h.fail);
      if(h.delay){await new Promise(resolve=>h.pending.push(resolve));}
      return {...h.physical};
    }});
  h.release=()=>{const all=h.pending.splice(0);all.forEach(r=>r());};
  return h;
}

test('before any measurement the factor is DPR; a measurement replaces it and reports the change once',async()=>{
  const h=host({dpr:1,view:[460,650],physical:{width:368,height:520}});
  assert.equal(h.scale.factor(),1);assert.equal(h.scale.ready(),false);
  close(await h.scale.refresh(),0.8);
  assert.equal(h.scale.ready(),true);close(h.scale.factor(),0.8);
  assert.deepEqual(h.changes.length,1);
  await h.scale.refresh();assert.equal(h.changes.length,1,'unchanged factor never re-reports');
  assert.deepEqual(h.scale.rect({getBoundingClientRect:()=>({left:10,top:20,width:30,height:40})}),[8,16,24,32]);
  assert.deepEqual(h.scale.point(100,50),[80,40]);
});

test('ensure() measures once per page state, so hot-rectangle reports cannot loop',async()=>{
  const h=host({dpr:1,view:[460,650],physical:{width:368,height:900}}); // inconsistent: rejected
  for(let i=0;i<50;i++){h.scale.ensure();await tick();}
  assert.equal(h.calls,3,'a rejected state is retried within one bounded run, then not by repeated reports');
  assert.equal(h.scale.factor(),1,'rejected measurement leaves DPR');assert.equal(h.changes.length,0);
  assert.equal(h.scale.ready(),false,'a rejected shape is never readiness');assert.equal(h.fails.length,1);
  h.physical={width:368,height:520};h.scale.invalidate();
  for(let i=0;i<50;i++){h.scale.ensure();await tick();}
  assert.equal(h.calls,4);close(h.scale.factor(),.8);
  for(let i=0;i<50;i++){h.scale.ensure();await tick();}
  assert.equal(h.calls,4,'an accepted state is not re-measured');
});

test('a resize during the native round trip discards that answer and measures the page as it is',async()=>{
  const h=host({dpr:1,view:[460,650],physical:{width:368,height:520}});h.delay=true;
  const done=h.scale.refresh();await tick();
  // WebKit zoom moves to .64 while the first answer is in flight.
  h.view=[575,812];h.scale.invalidate();h.release();await tick();await tick();h.release();
  close(await done,(368+520)/(575+812),'combined axes of the final state');assert.ok(Math.abs(h.scale.factor()-0.64)<0.001);assert.equal(h.calls,2);
  assert.equal(h.scale.ready(),true);
});

test('a viewport that returns to a measured size after a native resize is not trusted from before',async()=>{
  const h=host({dpr:1,view:[460,650],physical:{width:460,height:650}});
  close(await h.scale.refresh(),1);
  // The window grew to the .8 size and the zoom correction restored the 460 px viewport.
  h.physical={width:368,height:520};h.scale.invalidate();
  assert.equal(h.scale.ready(),false,'same viewport, new epoch: stale');
  close(await h.scale.refresh(),.8);
});

test('stale on a zooming engine keeps the measured zoom and follows DPR; WebView2 stays on DPR',async()=>{
  const g=host({dpr:1,view:[460,650],physical:{width:368,height:520}});await g.scale.refresh();
  g.dpr=2;assert.equal(g.scale.ready(),false);close(g.scale.factor(),1.6);
  const w=host({dpr:1.5,view:[460,650],physical:{width:690,height:975}});await w.scale.refresh();
  w.dpr=2;close(w.scale.factor(),2);
});

test('hosts without the API fall back to DPR; failing hosts stay unready without retry storms',async()=>{
  // Browser fixture: no measure function at all is the only "cannot say" (explicit legacy behaviour).
  const a=host({dpr:1.25,view:[460,650],physical:{width:1,height:1},absent:true});
  assert.equal(a.scale.ready(),true);assert.equal(a.scale.factor(),1.25);assert.equal(await a.scale.refresh(),1.25);assert.equal(a.scale.ensure(),false);
  const f=host({dpr:1,view:[460,650],physical:{width:368,height:520}});f.fail='denied';
  for(let i=0;i<20;i++){f.scale.ensure();await tick();}
  assert.equal(f.calls,3,'bounded: one run of three attempts');assert.equal(f.scale.factor(),1);assert.equal(f.scale.ready(),false);
  assert.equal(f.fails.length,1);assert.match(f.fails[0],/denied/);assert.equal(f.accepts.length,0);
});

test('a real API that answers null, undefined or a malformed size never becomes ready',async()=>{
  for(const answer of [null,undefined,{},{width:'368',height:null},{width:NaN,height:520},{width:-368,height:-520},'368x520',0]){
    let calls=0;const fails=[],accepts=[];
    const s=S.create({dpr:()=>1,viewport:()=>[460,650],wait:()=>Promise.resolve(),measure:async()=>{calls++;return answer;},
      onFail:r=>fails.push(r),onAccept:k=>accepts.push(k)});
    assert.equal(await s.refresh(),1,`${JSON.stringify(answer)}: DPR is reported, not adopted`);
    assert.equal(s.ready(),false,`${JSON.stringify(answer)} is not readiness`);
    assert.equal(s.state().unavailable,false,`${JSON.stringify(answer)} does not mark the host unable to say`);
    assert.equal(calls,3);assert.equal(fails.length,1);assert.equal(accepts.length,0);
    for(let i=0;i<10;i++){s.ensure();await tick();}
    assert.equal(calls,3,'no retry storm after the bounded run');
  }
});

test('an error then a valid answer within the bounded run is accepted; retries wait between attempts',async()=>{
  let calls=0;const waits=[];
  const s=S.create({dpr:()=>1,viewport:()=>[460,650],wait:ms=>{waits.push(ms);return Promise.resolve();},
    measure:async()=>{if(++calls<3)throw Error('busy');return{width:368,height:520};}});
  close(await s.refresh(),.8);assert.equal(s.ready(),true);assert.equal(calls,3);
  assert.equal(waits.length,2);assert.ok(waits[0]>0&&waits[1]>waits[0],'growing, bounded backoff');
  assert.ok(waits.reduce((a,b)=>a+b,0)<300,'well inside the native 1200 ms landing wait');
});

test('an accepted measurement reports once even when the factor is unchanged, and cannot loop',async()=>{
  // WebView2: k == DPR, so onChange never fires; a page that withheld its rectangles still needs one report.
  let reports=0,ensured=0;let s;
  s=S.create({dpr:()=>1.5,viewport:()=>[460,650],measure:async()=>({width:690,height:975}),
    onChange:()=>assert.fail('unchanged factor must not report a change'),
    onAccept:(k,changed)=>{reports++;assert.equal(k,1.5);assert.equal(changed,false);
      // reportHot calls ensure() and must find nothing to do.
      if(s.ensure())ensured++;}});
  for(let i=0;i<20;i++){s.ensure();await tick();}
  assert.equal(reports,1,'exactly one fresh report');assert.equal(ensured,0,'the report did not restart measurement');
  // A new page state measures again and reports once more, even with the same factor.
  s.invalidate();for(let i=0;i<20;i++){s.ensure();await tick();}
  assert.equal(reports,2);
});

test('an answer that arrives after the page changed on the last attempt is not accepted',async()=>{
  let view=460;const fails=[];const pending=[];
  const s=S.create({dpr:()=>1,viewport:()=>[view,650],wait:()=>Promise.resolve(),onFail:r=>fails.push(r),
    measure:()=>new Promise(resolve=>pending.push(resolve))});
  const done=s.refresh();
  for(let i=0;i<3;i++){await tick();view+=1;pending.shift()({width:368,height:520});}
  await done;
  assert.equal(s.ready(),false,'no stale answer adopted');assert.equal(s.state().measured,null);
  assert.equal(fails.length,1);assert.match(fails[0],/changed/);
});

test('a throwing callback does not turn an accepted measurement into a rejected promise',async()=>{
  const logs=[];
  const s=S.create({dpr:()=>1,viewport:()=>[460,650],measure:async()=>({width:368,height:520}),log:m=>logs.push(m),
    onChange:()=>{throw Error('boom');},onAccept:()=>{throw Error('bang');}});
  close(await s.refresh(),.8);assert.equal(s.ready(),true);
  assert.ok(logs.some(m=>/boom/.test(m))&&logs.some(m=>/bang/.test(m)));
});

test('key() names the page state: viewport, DPR and native epoch',()=>{
  let view=[460,650],dpr=1;
  const s=S.create({dpr:()=>dpr,viewport:()=>view,measure:async()=>({width:368,height:520})});
  const k0=s.key();assert.equal(s.key(),k0,'stable while nothing changes');
  s.invalidate();const k1=s.key();assert.notEqual(k1,k0,'native epoch');
  view=[575,812];const k2=s.key();assert.notEqual(k2,k1,'viewport');
  dpr=2;assert.notEqual(s.key(),k2,'DPR');
});

test('a measurement that never settles gives up after three attempts',async()=>{
  let view=0;const calls=[];
  const s=S.create({dpr:()=>1,viewport:()=>[460+view,650],measure:async()=>{calls.push(view);view++;return{width:368,height:520};}});
  await s.refresh();assert.equal(calls.length,3);assert.equal(s.ready(),false);
  for(let i=0;i<10;i++){s.ensure();await tick();}
  assert.equal(calls.length,3,'reports wait for the next resize/scale event');
  s.invalidate();s.ensure();await tick();await tick();await tick();await tick();
  assert.equal(calls.length,6,'an event allows one further bounded run');
});

/* The shipped page's use of the scale. Static contracts on notch.html only: they pin the wiring the
   review asked for (no guessed landing centre, withheld hot rectangles, a waiting press that cannot
   restart). Runtime behaviour on WebKitGTK/WebView2 still needs the browser and native runs. */
const notchSource=require('node:fs').readFileSync(require('node:path').join(__dirname,'../matra/ui/notch.html'),'utf8');
const body=(src,start,end)=>{const i=src.indexOf(start);assert.ok(i>=0,`missing ${start}`);const j=src.indexOf(end,i+start.length);assert.ok(j>i,`missing end of ${start}`);return src.slice(i,j);};

test('notch.html: a carry landing without a valid scale is cancelled, never acknowledged with a guess',()=>{
  const handler=body(notchSource,"listen('carry_layout'",'}).catch(()=>{});');
  assert.match(handler,/settledScale\(\)\.then\(ok=>/,'the settled result is read');
  const check=handler.indexOf('if(!ok||!nativeScale.ready())return cancelLanding('),ack=handler.indexOf("invoke('carry_layout_ready'");
  assert.ok(check>0&&ack>check,'the failure check precedes the only acknowledgement');
  assert.match(body(handler,'const cancelLanding=','};'),/releaseCarry\(true\)/,'the existing carry cancel path');
  assert.match(handler,/\}\)\.catch\(error=>cancelLanding\(/,'a rejected promise also cancels');
  assert.equal((handler.match(/carry_layout_ready/g)||[]).length,1);
});

test('notch.html: hot rectangles are withheld until the scale is ready and reported once it is',()=>{
  const report=body(notchSource,'function reportHot(){','\n}\n');
  const gate=report.indexOf('if(!nativeScale.ready())return;'),body1=report.indexOf("invoke('report_notch_body'"),carry=report.indexOf('if(carrying||dragging)');
  assert.ok(gate>0,'readiness gate');
  assert.ok(body1>=0&&body1<gate,'report_notch_body (physical-independent layout) is still reported before the gate');
  assert.ok(carry<gate,'carry keeps placing its handles');
  for(const use of ['publishHot(','rectOf(','wakeRect()','nativeScale.box(','nativeScale.factor()'])
    assert.ok(report.indexOf(use)<0||report.indexOf(use)>gate,`${use} only after the gate`);
  assert.doesNotMatch(report,/callq\('set_hot'|invoke\('set_hot'/,'all publication goes through publishHot');
  assert.match(notchSource,/onAccept:\(\)=>reportHot\(\)/,'an accepted measurement reports once, changed factor or not');
  assert.match(notchSource,/onFail:reason=>withdrawHot\(/,'a failed measurement withdraws stale rectangles');
  assert.match(body(notchSource,'function withdrawHot(','\n}\n'),/if\(!hotPublished\)return;[\s\S]*set_hot',\{rects:\[\],expanded:false\}/);
  assert.doesNotMatch(report,/invalidate\(|remeasureScale\(/,'reporting never invalidates, so it cannot loop');
});

test('notch.html: a press waiting for the scale is abandoned on changed geometry and cannot restart',()=>{
  const cancel=body(notchSource,'function cancelPendingCarry(){','}\n');
  assert.match(cancel,/pendingCarry=null;press=null;/,'abandoning forgets the press');
  const start=body(notchSource,'function startCarry(command,point,grip){','\n}\n');
  assert.match(start,/key:nativeScale\.key\(\)/,'the page state is captured with the point');
  assert.match(start,/if\(nativeScale\.key\(\)!==pending\.key\)/,'and revalidated before use');
  assert.match(start,/if\(!ok\)return abandon\(/);assert.match(start,/\.catch\(error=>abandon\(/);
  for(const [name,pattern] of [['Escape',/key==='Escape'\)\{cancelPendingCarry\(\);/],['pointercancel',/'pointercancel',\(\)=>\{cancelPendingCarry\(\);releaseCarry\(true\);/],
    ['blur',/'blur',\(\)=>\{cancelPendingCarry\(\);if\(carryLifecycle\.active\(\)\)releaseCarry\(true\);/],['pagehide',/'pagehide',\(\)=>\{cancelPendingCarry\(\);releaseCarry\(true\);/]])
    assert.match(notchSource,pattern,name);
  // mouse-up: pending abandoned first; an active carry is released (not cancelled) as before.
  assert.match(body(notchSource,"document.addEventListener('mouseup'",'\n});'),/cancelPendingCarry\(\);\s*if\(carryLifecycle\.active\(\)\)releaseCarry\(false\);/);
});
