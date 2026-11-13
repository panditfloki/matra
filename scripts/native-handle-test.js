// Actual packaged WebView2 geometry and IPC. Synthetic fixtures stay on disposable CI.
const {execFileSync} = require('node:child_process');
const path = require('node:path');
const assert = require('node:assert/strict');
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
function evaluate(js) {
  return JSON.parse(execFileSync(process.execPath, [path.join(__dirname, 'native-inspect.js'), 'notch', js], {encoding:'utf8', timeout:30000})).value;
}
function disposable() {
  if (process.env.GITHUB_ACTIONS !== 'true' || process.platform !== 'win32') throw new Error('Synthetic native geometry tests require a disposable Windows CI runner');
}

// Self-contained when serialized into WebView2 or the inert browser fixture.
// Mousemove listeners drive reveal; set_hot still reaches the original bridge.
async function probeHandles() {
  const reference=window.MatraNotchReference;
  if(!reference||!reference.reference)throw new Error('Mac notch reference is not loaded in this page');
  const saved={showMove,folded,hovered,dockActive,pointerIn,onHover,carrying,dragging,press,hoverId,handleReturnUntil,
    cardShown:card.classList.contains('show'),scrollLeft:pill.scrollLeft,scrollTop:pill.scrollTop};
  const motion=document.createElement('style');
  motion.textContent='#root #pill,#root #rest,#root .cell,#root .handle,#root .handle>*{transition:none!important;animation:none!important}';
  document.head.appendChild(motion);
  const oldCall=callq,pending=[];let last;
  callq=(name,args)=>{
    const result=oldCall(name,args);
    if(name==='set_hot'){last=JSON.parse(JSON.stringify(args));pending.push(result);}
    return result;
  };
  try {
    onHover=false;carrying=false;dragging=false;press=null;pointerIn=false;
    clearTimeout(hideTimer);hideCard();unfold();showMove=true;placeHandles();setDockActive(false);setHovered(null);
    pill.scrollLeft=0;pill.scrollTop=0;
    const box=el=>{const r=el.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
    const p=box(pill),o=box(orb),m=box(moveHandle),vertical=edgeIsVertical();
    const zoom=parseFloat(document.documentElement.style.zoom)||1,dpr=devicePixelRatio||1;
    const cx=p.left+p.width/2,cy=p.top+p.height/2;
    const oc={x:o.left+o.width/2,y:o.top+o.height/2},mc={x:m.left+m.width/2,y:m.top+m.height/2};
    const control=el=>{
      const css=getComputedStyle(el),r=box(el),present=css.display!=='none'&&css.visibility!=='hidden'&&r.width>0&&r.height>0;
      const part=selector=>{
        const style=getComputedStyle(el.querySelector(selector)),matrix=new DOMMatrixReadOnly(style.transform);
        return {visible:present&&Number(css.opacity)*Number(style.opacity)>.99&&style.display!=='none'&&style.visibility!=='hidden',
          width:parseFloat(style.width),height:parseFloat(style.height),scale:Math.hypot(matrix.a,matrix.b)};
      };
      return {present,pointer:css.pointerEvents,glyph:part('.h-glyph'),disc:part('.h-disc'),rest:part('.h-rest')};
    };
    const snapshot=()=>{reportHot();return {controls:[orb,moveHandle].map(control),hot:last.rects,expanded:last.expanded,dockActive,hovered};};
    const point=(x,y)=>{
      document.dispatchEvent(new MouseEvent('mousemove',{clientX:x,clientY:y,bubbles:true}));
      clearTimeout(hideTimer);hideCard();return {direct:onHandle(x,y),intent:handleIntent(x,y),...snapshot()};
    };
    const axisPoint=along=>vertical?[cx,along]:[along,cy],handleAxisPoint=along=>vertical?[oc.x,along]:[along,oc.y];
    const near=vertical?p.top+zoom:p.left+zoom,far=vertical?p.bottom-zoom:p.right-zoom;
    const end=vertical?p.bottom:p.right,orbNear=vertical?o.top:o.left;
    const states={idle:snapshot(),center:point(cx,cy),near:point(...axisPoint(near)),far:point(...axisPoint(far)),
      firstGap:point(...handleAxisPoint((end+orbNear)/2)),orb:point(oc.x,oc.y),
      overlap:point((oc.x+mc.x)/2,(oc.y+mc.y)/2),grip:point(mc.x,mc.y),away:point(...axisPoint(near-20*zoom))};
    setDockActive(false);setHovered(null);states.beforeDiscovery=snapshot();
    states.gripDiscovery=point(mc.x,mc.y);
    setDockActive(false);setHovered(null);states.orbDiscovery=point(oc.x,oc.y);
    // Square corners, near-circle rims and overlap, excluding the wider travel
    // lane. Node calculates expected ownership from independent Mac anchors.
    const targetSamples=[];
    for(const r of [o,m])for(const a of [-1.05,-.8,0,.8,1.05])for(const b of [-1.05,-.8,0,.8,1.05]) {
      const x=r.left+r.width*(1+a)/2,y=r.top+r.height*(1+b)/2;
      targetSamples.push({x,y,direct:onHandle(x,y)});
    }
    for(const r of [o,m])for(const a of [-1.01,-.99,.99,1.01]) {
      const x=r.left+r.width*(1+a)/2,y=r.top+r.height/2;
      targetSamples.push({x,y,direct:onHandle(x,y)});
    }
    targetSamples.push({x:(oc.x+mc.x)/2,y:(oc.y+mc.y)/2,direct:onHandle((oc.x+mc.x)/2,(oc.y+mc.y)/2)});
    const dots=[...moveHandle.querySelectorAll('.h-glyph circle')].map(el=>({x:Number(el.getAttribute('cx')),y:Number(el.getAttribute('cy')),r:Number(el.getAttribute('r'))}));
    showMove=false;placeHandles();setHovered(null);
    states.moveOff=point(oc.x,oc.y);states.formerGrip=point(mc.x,mc.y);
    const moveDisabled={at:moveAt,display:getComputedStyle(moveHandle).display};
    showMove=true;placeHandles();setHovered('move');setFolded(true);states.folded=snapshot();
    const foldedTargets=[oc,mc].map(({x,y})=>({direct:onHandle(x,y),intent:handleIntent(x,y)}));
    const rest=box(document.querySelector('#rest'));
    setFolded(false);setHovered(null);placeHandles();reportHot();
    const cells=[...pill.querySelectorAll('.cell')],scrollKey=vertical?'scrollTop':'scrollLeft';
    const content=vertical?pill.scrollHeight:pill.scrollWidth,client=vertical?pill.clientHeight:pill.clientWidth;
    const firstAtStart=box(cells[0]),lastAtStart=box(cells[cells.length-1]),ring=box(cells[0].querySelector('.ringwrap'));
    pill[scrollKey]=content-client;
    const scroll={maximum:Math.max(0,content-client),end:pill[scrollKey],firstAtStart,lastAtStart,
      firstAtEnd:box(cells[0]),lastAtEnd:box(cells[cells.length-1]),overflowStyle:vertical?getComputedStyle(pill).overflowY:getComputedStyle(pill).overflowX};
    pill[scrollKey]=0;
    await Promise.all(pending);
    return {referenceKind:reference.reference.kind,edge:notchEdge,vertical,zoom,dpr,pill:p,orb:o,grip:m,rest,ring,states,moveDisabled,foldedTargets,targetSamples,dots,scroll,
      gap:parseFloat(getComputedStyle(pill).gap),lineHeight:document.getElementById('notch-percent-measure').offsetHeight,
      cellExtent:parseFloat(getComputedStyle(cells[0]).height),insets:[...insets],
      viewport:{left:0,top:0,right:innerWidth,bottom:innerHeight},
      usable:{left:insets[3]*zoom,top:insets[0]*zoom,right:innerWidth-insets[1]*zoom,bottom:innerHeight-insets[2]*zoom},
      cells:cells.length,overflow:content>client};
  } finally {
    callq=oldCall;clearTimeout(hideTimer);clearTimeout(dockTimer);
    try {
      showMove=saved.showMove;onHover=saved.onHover;carrying=saved.carrying;dragging=saved.dragging;press=saved.press;pointerIn=saved.pointerIn;hoverId=saved.hoverId;
      setFolded(saved.folded);setDockActive(saved.dockActive);setHovered(saved.hovered);handleReturnUntil=saved.handleReturnUntil;
      pill.scrollLeft=saved.scrollLeft;pill.scrollTop=saved.scrollTop;
      if(saved.cardShown)showCard();else hideCard();
      reportHot();
    } finally { motion.remove(); }
  }
}

function assertHandles(value,context='native handles') {
  const {pill:p,orb:o,grip:m,vertical:v,zoom:z,dpr,states:s}=value;
  const near=r=>v?r.top:r.left,far=r=>v?r.bottom:r.right,along=r=>(near(r)+far(r))/2,across=r=>v?r.left+r.width/2:r.top+r.height/2;
  const close=(actual,expected,label,tolerance=.2)=>assert.ok(Number.isFinite(actual)&&Math.abs(actual-expected)<=tolerance,`${context}: ${label}, ${actual} versus ${expected}`);
  // NotchLayout/NotchViewModel design anchors. The adapter never supplies the
  // expected geometry, and AppKit's platform-measured line height is not guessed.
  const q=44/117,flare=103*q,orbRadius=76*q,gripRadius=70*q,separation=100*q;
  assert.equal(value.referenceKind,'mac-notch-reference-data',`${context}: Mac reference loaded`);
  assert.equal(v,['left','right'].includes(value.edge),`${context}: edge orientation`);
  assert.ok(z>0&&dpr>0&&value.cells>0&&value.lineHeight>0,`${context}: usable render measurements`);
  close((along(o)-far(p))/z,flare,'settings center at trailing flare');
  close((along(m)-along(o))/z,separation,'settings-to-grip center distance');
  const expectedAcross=value.edge==='right'?p.right-flare*z:value.edge==='left'?p.left+flare*z:
    value.edge==='top'?p.top+flare*z:p.bottom-flare*z;
  close(across(o),expectedAcross,'settings is one flare inward');close(across(m),expectedAcross,'grip shares settings axis');
  close(o.width/z,152*q,'settings target diameter');close(o.height/z,152*q,'settings target height');
  close(m.width/z,140*q,'grip target diameter');close(m.height/z,140*q,'grip target height');
  assert.ok(near(m)<far(o)&&along(m)>along(o),`${context}: circular targets overlap with settings first`);
  close(along(value.rest),along(p),'resting lip is centered on inset-adjusted body');
  close((v?value.rest.width:value.rest.height)/z,26*q,'resting lip depth');
  close((v?value.rest.height:value.rest.width)/z,210*q,'resting lip length');
  close(value.ring.width/z,44,'native ring diameter');close(value.ring.height/z,44,'native ring height');
  close(value.cellExtent,44+26.9*q+value.lineHeight,'fixed provider cell includes measured percentage line');
  close((v?p.width:p.height)/z,186*q+(v?0:26.9*q+value.lineHeight),'native body depth');
  // The fixed Windows overlay reserves each end for flare + handle separation +
  // grip radius: (103 + 100 + 70) design pixels. Vertical gaps compress first.
  const available=Math.max(44,(v?value.viewport.bottom:value.viewport.right)/z-(v?value.insets[0]+value.insets[2]:value.insets[1]+value.insets[3])-2*273*q);
  const packed=119.6*q+value.cells*(v?44+26.9*q+value.lineHeight:44);
  const spacing=v&&value.cells>1?Math.min(83.5*q,Math.max(0,(available-packed)/(value.cells-1))):83.5*q;
  close(value.gap,spacing,'spacing compresses only on vertical edges');
  close((far(p)-near(p))/z,Math.min(available,packed+Math.max(0,value.cells-1)*spacing),'body fits available overlay length');
  for(const [name,r] of [['body',p],['settings target',o],['grip target',m],['resting lip',value.rest]])for(const [kind,bounds] of [['viewport',value.viewport],['usable area',value.usable]]) {
    assert.ok(r.left>=bounds.left-1&&r.top>=bounds.top-1&&r.right<=bounds.right+1&&r.bottom<=bounds.bottom+1,`${context}: ${name} fits ${kind}: ${JSON.stringify({r,bounds})}`);
  }
  assert.ok(near(p)-flare*z>=near(value.usable)-1&&far(p)+flare*z<=far(value.usable)+1,`${context}: nominal flares fit usable span`);
  const visible=(state,expected,label)=>assert.deepEqual(state.controls.map(control=>control.glyph.visible),expected,`${context}: ${label}`);
  for(const name of ['idle','center','near','away','beforeDiscovery']) {
    visible(s[name],[false,false],`${name} hides both glyphs`);
    assert.equal(s[name].controls[0].rest.visible,true,`${context}: ${name} retains resting settings arc`);
    assert.equal(s[name].controls[0].disc.visible,false,`${context}: ${name} has no settings disc`);
    for(const control of s[name].controls) {
      assert.equal(control.present,true,`${context}: ${name} retains discoverable targets`);
      assert.equal(control.pointer,'auto',`${context}: ${name} target remains interactive`);
    }
  }
  for(const name of ['center','near','away'])assert.equal(s[name].intent,null,`${context}: ${name} does not reveal handles`);
  for(const name of ['far','firstGap','orb','overlap','grip','gripDiscovery','orbDiscovery']) {
    visible(s[name],[true,true],`${name} reveals pair together`);
    assert.equal(s[name].controls[0].disc.visible,true,`${context}: ${name} shows settings disc`);
    assert.equal(s[name].controls[0].rest.visible,false,`${context}: ${name} hides resting arc`);
  }
  for(const name of ['far','firstGap','orb','overlap','orbDiscovery'])assert.equal(s[name].intent,'orb',`${context}: ${name} reveals settings`);
  for(const name of ['grip','gripDiscovery'])assert.equal(s[name].intent,'move',`${context}: ${name} discovers grip`);
  assert.equal(s.beforeDiscovery.dockActive,false,`${context}: discovery begins outside active dock`);
  assert.equal(s.gripDiscovery.direct,'move',`${context}: hidden grip is directly discoverable`);
  assert.equal(s.overlap.direct,'orb',`${context}: settings wins overlap`);
  const expectedOrb=v?{x:expectedAcross,y:p.bottom+flare*z}:{x:p.right+flare*z,y:expectedAcross};
  const expectedMove={x:expectedOrb.x+(v?0:separation*z),y:expectedOrb.y+(v?separation*z:0)};
  let outsideSamples=0,overlapSamples=0;
  for(const sample of value.targetSamples) {
    const inOrb=Math.hypot(sample.x-expectedOrb.x,sample.y-expectedOrb.y)<=orbRadius*z;
    const inMove=Math.hypot(sample.x-expectedMove.x,sample.y-expectedMove.y)<=gripRadius*z;
    outsideSamples+=!inOrb&&!inMove;overlapSamples+=inOrb&&inMove;
    assert.equal(sample.direct,inOrb?'orb':inMove?'move':null,`${context}: circular ownership at ${sample.x},${sample.y}`);
  }
  assert.ok(outsideSamples>0&&overlapSamples>0,`${context}: probes cover circle exteriors and overlap`);
  close(s.orb.controls[0].disc.width,124*q,'settings disc diameter');close(s.orb.controls[0].glyph.width,56*q,'gear size');
  close(s.orb.controls[1].glyph.scale,1,'grip size over settings');close(s.grip.controls[1].glyph.scale,1.3,'grip grows 30 percent');
  assert.equal(value.dots.length,6,`${context}: six grip dots`);
  const expectedDots=[];
  for(const a of [-.5,.5])for(const b of [-1,0,1])expectedDots.push({x:(v?b:a)*31*q,y:(v?a:b)*31*q,r:19*q/2});
  value.dots.forEach((dot,i)=>{for(const axis of ['x','y','r'])close(dot[axis],expectedDots[i][axis],`grip dot ${i} ${axis}`);});
  for(const [name,state] of Object.entries(s))assert.equal(state.controls[1].disc.visible,false,`${context}: ${name} keeps bare dots without grip disc`);
  const physical=r=>[r.left*dpr,r.top*dpr,r.width*dpr,r.height*dpr];
  const bridge=r=>v?[r.left*dpr,p.bottom*dpr,r.width*dpr,(r.top-p.bottom)*dpr]:[p.right*dpr,r.top*dpr,(r.left-p.right)*dpr,r.height*dpr];
  const expectedHot=[physical(p),physical(m),bridge(m),physical(o),bridge(o)];
  const exactHot=(state,expected,label)=>{
    assert.equal(state.hot.length,expected.length,`${context}: ${label} exact hot rectangle count`);
    const unused=[...state.hot];
    for(const rect of expected) {
      assert.ok(rect.every(Number.isFinite)&&rect[2]>0&&rect[3]>0,`${context}: ${label} positive physical rectangle`);
      const at=unused.findIndex(actual=>actual.length===4&&actual.every((n,i)=>Math.abs(n-rect[i])<1e-6));
      assert.ok(at>=0,`${context}: ${label} exact physical rectangle ${JSON.stringify(rect)} in ${JSON.stringify(state.hot)}`);
      unused.splice(at,1);
    }
    assert.equal(state.expanded,false,`${context}: handle reveal does not report open card`);
  };
  for(const name of ['idle','center','near','far','firstGap','orb','overlap','grip','away','beforeDiscovery','gripDiscovery','orbDiscovery'])exactHot(s[name],expectedHot,name);
  const hot=s.idle.hot,hit=(x,y,rects=hot)=>rects.some(r=>x>=r[0]&&x<=r[0]+r[2]&&y>=r[1]&&y<=r[1]+r[3]);
  for(let position=far(p)+.25*z;position<far(m);position+=.5*z) {
    const [x,y]=v?[across(o),position]:[position,across(o)];
    assert.ok(hit(x*dpr,y*dpr),`${context}: continuous body-to-grip native travel at ${position}`);
  }
  const offAxis=Math.min(v?o.left:o.top,v?m.left:m.top)-2*z,gapAlong=(far(p)+near(o))/2;
  assert.equal(hit(...(v?[offAxis*dpr,gapAlong*dpr]:[gapAlong*dpr,offAxis*dpr])),false,`${context}: transparent space beside bridge is absent from hot payload`);
  visible(s.moveOff,[true,false],'disabled move retains settings alone');
  exactHot(s.moveOff,[physical(p),physical(o),bridge(o)],'move disabled');
  exactHot(s.formerGrip,[physical(p),physical(o),bridge(o)],'former grip');
  assert.equal(value.moveDisabled.at,null);assert.equal(value.moveDisabled.display,'none');
  assert.equal(s.formerGrip.direct,null);assert.equal(s.formerGrip.intent,null);visible(s.formerGrip,[false,false],'former grip does not reveal controls');
  assert.equal(hit((m.left+m.width/2)*dpr,(m.top+m.height/2)*dpr,s.moveOff.hot),false,`${context}: disabled grip absent from native hot payload`);
  visible(s.folded,[false,false],'folded notch hides both controls');
  assert.deepEqual(value.foldedTargets,[{direct:null,intent:null},{direct:null,intent:null}],`${context}: folded targets cannot be discovered`);
  for(const control of s.folded.controls)assert.equal(control.pointer,'none',`${context}: folded target has no pointer events`);
  const wake={...value.rest},band=90*q*z;
  if(value.edge==='left')wake.width+=band;
  else if(value.edge==='right'){wake.left-=band;wake.width+=band;}
  else if(value.edge==='top')wake.height+=band;
  else {wake.top-=band;wake.height+=band;}
  exactHot(s.folded,[physical(wake)],'folded wake band');
  const scroll=value.scroll;
  assert.equal(value.overflow,scroll.maximum>0,`${context}: overflow measurement`);
  if(value.overflow) {
    assert.ok(['auto','scroll'].includes(scroll.overflowStyle),`${context}: long list remains scrollable`);
    close(scroll.end,scroll.maximum,'scroll reaches far limit',1);
    close((near(scroll.firstAtStart)-near(scroll.firstAtEnd))/z,scroll.end,'scroll moves retained providers',1);
    assert.ok(near(scroll.firstAtStart)>=near(p)-1&&far(scroll.firstAtStart)<=far(p)+1,`${context}: first provider fully reachable`);
    assert.ok(near(scroll.lastAtEnd)>=near(p)-1&&far(scroll.lastAtEnd)<=far(p)+1,`${context}: last provider fully reachable`);
    assert.ok(far(scroll.lastAtStart)>far(p),`${context}: scroll exposes initially clipped final provider`);
  }
}

async function testNativeHandles(run=evaluate,{scales=[1],counts=[6,10],insetCases=[[0,0,0,0],'far']}={}) {
  disposable();
  const original=run("Promise.all([invoke('get_scale'),invoke('get_notch_edge')])");
  run(`window.__matraHandleContractState={providers,activity,onHover,showMove,pointerIn,carrying,hovered,dockActive,folded,insets:[...insets],hoverId,cardShown:card.classList.contains('show'),transitions:[orb.style.transition,moveHandle.style.transition,controlsCue.style.transition]};onHover=false;activity=[];carrying=false;orb.style.transition='none';moveHandle.style.transition='none';controlsCue.style.transition='none';hideCard();unfold();`);
  const results=[];
  try {
    for(const scale of scales)for(const edge of ['left','right','top','bottom']) {
      run(`invoke('set_scale',{scale:${scale}}).then(()=>invoke('set_notch_edge',{edge:${JSON.stringify(edge)}}))`);
      await pause(650);
      for(const count of counts)for(const requestedInset of insetCases) {
        // Native placement already pins to the work-area edge. Only taskbar
        // overlap along the capsule's long axis reaches this renderer.
        const inset=requestedInset==='far'?(['left','right'].includes(edge)?[0,0,60,0]:[0,60,0,0]):requestedInset;
        run(`providers=()=>Array.from({length:${count}},(_,i)=>({id:'handle-fixture-'+i,base:['claude','codex','glm','cursor','grok','gemini'][i%6],name:'Synthetic provider '+i,glyph:'F',snap:{status:'ok',fetched_at:Date.now(),windows:[{id:'session',label:'Current session',used:.2}]}}));hoverId=null;applyInsets(${JSON.stringify(inset)});showMove=true;renderRing();unfold();setDockActive(true);`);
        const value=run(`(${probeHandles.toString()})()`), context=`${edge}, scale=${scale}, cells=${count}, insets=${inset}`;
        assert.equal(value.edge,edge);assert.equal(value.cells,count);assertHandles(value,context);
        if(count>=10)assert.equal(value.overflow,true,`${context}: large fixture exercises scrolling rather than losing providers`);
        results.push({edge,scale,count,inset,overflow:value.overflow,viewport:value.viewport});
      }
    }
    return {passed:true,cases:results.length,results};
  } finally {
    try { run(`invoke('set_scale',{scale:${JSON.stringify(original[0])}}).then(()=>invoke('set_notch_edge',{edge:${JSON.stringify(original[1])}}))`);await pause(650); }
    finally { run(`(()=>{const s=window.__matraHandleContractState;providers=s.providers;activity=s.activity;onHover=s.onHover;showMove=s.showMove;pointerIn=s.pointerIn;carrying=s.carrying;hoverId=s.hoverId;applyInsets(s.insets);renderRing();setFolded(s.folded);setDockActive(s.dockActive);setHovered(s.hovered);[orb.style.transition,moveHandle.style.transition,controlsCue.style.transition]=s.transitions;if(s.cardShown)showCard();else hideCard();reportHot();delete window.__matraHandleContractState;})()`); }
  }
}
module.exports={testNativeHandles,evaluate,probeHandles,assertHandles};
if(require.main===module)testNativeHandles().then(result=>console.log(JSON.stringify(result,null,2))).catch(error=>{console.error(error);process.exitCode=1;});
