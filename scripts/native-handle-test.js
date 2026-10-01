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

// Mousemove listeners drive the reveal. Capture and forward real set_hot IPC.
async function probeHandles() {
  const oldCall = callq, pending = []; let last;
  callq = (name, args) => {
    const result = oldCall(name, args);
    if (name === 'set_hot') { last = JSON.parse(JSON.stringify(args)); pending.push(result); }
    return result;
  };
  try {
    hideCard(); unfold(); setDockActive(true); showMove=true; placeHandles(); setHovered(null);
    const box = el => {const r=el.getBoundingClientRect();return {left:r.left,top:r.top,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
    const p=box(pill), o=box(orb), m=box(moveHandle), vertical=edgeIsVertical();
    const zoom=parseFloat(document.documentElement.style.zoom)||1, dpr=devicePixelRatio||1;
    const cx=p.left+p.width/2, cy=p.top+p.height/2;
    const snapshot = () => {
      reportHot();
      return {controls:[orb,moveHandle].map(el=>{const css=getComputedStyle(el);return {visible:css.display!=='none'&&Number(css.opacity)>.99,pointer:css.pointerEvents};}),hot:last.rects};
    };
    const point = (x,y) => {
      document.dispatchEvent(new MouseEvent('mousemove',{clientX:x,clientY:y,bubbles:true}));
      hideCard(); return {intent:handleIntent(x,y),...snapshot()};
    };
    const axisPoint = along => vertical?[cx,along]:[along,cy];
    const near=vertical?p.top+zoom:p.left+zoom, far=vertical?p.bottom-zoom:p.right-zoom;
    const end=vertical?p.bottom:p.right, orbNear=vertical?o.top:o.left, orbFar=vertical?o.bottom:o.right, gripNear=vertical?m.top:m.left;
    const states={idle:snapshot(),center:point(cx,cy),near:point(...axisPoint(near)),far:point(...axisPoint(far)),
      firstGap:point(...axisPoint((end+orbNear)/2)),orb:point(o.left+o.width/2,o.top+o.height/2),
      secondGap:point(...axisPoint((orbFar+gripNear)/2)),grip:point(m.left+m.width/2,m.top+m.height/2),
      away:point(...axisPoint(near-20*zoom))};
    showMove=false;placeHandles();
    states.moveOff=point(...axisPoint(far));
    states.formerGrip=point(m.left+m.width/2,m.top+m.height/2);
    const moveDisabled={at:moveAt,display:getComputedStyle(moveHandle).display};
    setFolded(true);states.folded=snapshot();
    const foldedIntent=handleIntent(o.left+o.width/2,o.top+o.height/2);
    setFolded(false);showMove=true;setHovered(null);placeHandles();reportHot();
    await Promise.all(pending);
    return {edge:notchEdge,vertical,zoom,dpr,pill:p,orb:o,grip:m,rest:box(document.querySelector('#rest')),states,moveDisabled,foldedIntent,
      gap:getComputedStyle(pill).gap,depth:vertical?pill.offsetWidth:pill.offsetHeight,
      lip:Math.min(document.querySelector('#rest').offsetWidth,document.querySelector('#rest').offsetHeight),
      viewport:{left:0,top:0,right:innerWidth,bottom:innerHeight},
      usable:{left:insets[3]*zoom,top:insets[0]*zoom,right:innerWidth-insets[1]*zoom,bottom:innerHeight-insets[2]*zoom},
      cells:pill.querySelectorAll('.cell').length,overflow:vertical?pill.scrollHeight>pill.clientHeight:pill.scrollWidth>pill.clientWidth};
  } finally { callq=oldCall;setFolded(false);setHovered(null);reportHot(); }
}

function assertHandles(value, context) {
  const {pill:p,orb:o,grip:m,vertical:v,zoom:z,dpr,states:s}=value;
  const near=r=>v?r.top:r.left, far=r=>v?r.bottom:r.right, center=r=>v?r.left+r.width/2:r.top+r.height/2;
  const close=(actual,expected,label)=>assert.ok(Math.abs(actual-expected)<=.75, `${context}: ${label}, ${actual} versus ${expected}`);
  close((near(o)-far(p))/z,2,'settings begins after the far capsule end');
  close((near(m)-far(o))/z,2,'grip follows settings at the same end');
  close(center(o),center(p),'settings shares capsule axis');close(center(m),center(p),'grip shares capsule axis');
  close(v?value.rest.top+value.rest.height/2:value.rest.left+value.rest.width/2,v?p.top+p.height/2:p.left+p.width/2,'resting lip remains centered on the inset-adjusted capsule');
  close(o.width/z,32,'settings target width');close(m.width/z,32,'grip target width');
  assert.ok(near(o)>far(p)&&near(m)>far(o), `${context}: both controls are beyond the far end, gear before grip`);
  for(const [name,r] of [['capsule',p],['settings',o],['grip',m]])for(const [kind,bounds] of [['viewport',value.viewport],['usable area',value.usable]]) {
    assert.ok(r.left>=bounds.left-1&&r.top>=bounds.top-1&&r.right<=bounds.right+1&&r.bottom<=bounds.bottom+1, `${context}: ${name} fits ${kind}: ${JSON.stringify({r,bounds})}`);
  }
  const visible = (state,expected,label) => assert.deepEqual(state.controls.map(control=>control.visible),expected,`${context}: ${label}`);
  for(const name of ['idle','center','near','away']) {
    visible(s[name],[false,false],`${name} hides both controls`);
    assert.ok(s[name].controls.every(control=>control.pointer==='none'),`${context}: ${name} has no hidden pointer targets`);
    assert.equal(s[name].hot.length,1,`${context}: ${name} reports capsule only`);
  }
  assert.equal(s.center.intent,null);assert.equal(s.near.intent,null);assert.equal(s.away.intent,null);
  for(const name of ['far','firstGap','orb','secondGap','grip']) {
    visible(s[name],[true,true],`${name} reveals the pair together`);
    assert.equal(s[name].hot.length,5,`${context}: ${name} reports capsule, both controls and their travel lanes`);
  }
  assert.equal(s.far.intent,'orb');assert.equal(s.firstGap.intent,'orb');assert.equal(s.orb.intent,'orb');
  assert.equal(s.secondGap.intent,'move');assert.equal(s.grip.intent,'move');
  const hot=s.far.hot, hit=(x,y,rects=hot)=>rects.some(r=>x>=r[0]&&x<=r[0]+r[2]&&y>=r[1]&&y<=r[1]+r[3]);
  const physical = r=>[r.left*dpr,r.top*dpr,r.width*dpr,r.height*dpr];
  for(const r of [o,m])assert.ok(hot.some(h=>h.every((n,i)=>Math.abs(n-physical(r)[i])<.75)),`${context}: rendered control is reported in physical pixels`);
  const surfaces=[p,o,m].map(physical), lanes=hot.filter(r=>!surfaces.some(surface=>r.every((n,i)=>Math.abs(n-surface[i])<.75)));
  assert.equal(lanes.length,2,`${context}: only explicit travel lanes accompany the surfaces`);
  for(const lane of lanes) {
    close((v?lane[2]:lane[3])/(z*dpr),32,'travel lane is only as wide as a control');
    assert.ok(lane[2]>0&&lane[3]>0,`${context}: travel lane has positive dimensions`);
  }
  for(let along=far(p)+.25*z;along<far(m);along+=.5*z) {
    const [x,y]=v?[center(p),along]:[along,center(p)];
    assert.ok(hit(x*dpr,y*dpr),`${context}: continuous capsule-to-grip native travel at ${along}`);
  }
  const offAxis=center(p)+18*z, gapAlong=(far(p)+near(o))/2;
  assert.equal(hit(...(v?[offAxis*dpr,gapAlong*dpr]:[gapAlong*dpr,offAxis*dpr])),false,`${context}: transparent space beside bridge is absent from exact hot payload`);
  visible(s.moveOff,[true,false],'disabled move retains settings alone');
  assert.equal(s.moveOff.hot.length,3);assert.equal(value.moveDisabled.at,null);assert.equal(value.moveDisabled.display,'none');
  assert.equal(s.formerGrip.intent,null);visible(s.formerGrip,[false,false],'old grip position does not reveal controls');
  assert.equal(hit((m.left+m.width/2)*dpr,(m.top+m.height/2)*dpr,s.moveOff.hot),false,'disabled grip is absent from native hot rectangles');
  visible(s.folded,[false,false],'folded notch hides both controls');assert.equal(value.foldedIntent,null);
  assert.equal(s.folded.hot.length,1);assert.equal(value.lip,5);assert.equal(value.gap,'10px');
  if(v)assert.equal(value.depth,62);
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
module.exports={testNativeHandles,evaluate};
if(require.main===module)testNativeHandles().then(result=>console.log(JSON.stringify(result,null,2))).catch(error=>{console.error(error);process.exitCode=1;});
