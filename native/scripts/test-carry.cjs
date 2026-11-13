const test=require('node:test'),assert=require('node:assert/strict');
const Carry=require('../matra/ui/matra-carry.js'),Notch=require('../matra/ui/matra-notch-reference.js');
test('straight carry body is the actual ordinary Mac-derived silhouette',()=>{
  for(const edge of Carry.edges)for(const depth of [Notch.reference.layout.sideBodyDepth,95])for(const length of [120,440]){
    const ordinary=Notch.silhouette(edge,depth,length,2);
    const carry=Carry.bodyPath(edge,depth,length+2*Notch.reference.layout.curlRadius,Notch.reference.layout.curlRadius,Notch.reference.layout.cornerRadius,2);
    assert.equal(carry,ordinary.path,edge);
  }
});
test('joined corner parts stay finite through all four corners and both directions',()=>{
  const p={scale:1,flare:38.735,corner_radius:29.634,bleed:2,layouts:Carry.edges.map(()=>({depth:70}))};
  for(const corner of ['topRight','bottomRight','bottomLeft','topLeft'])for(let before=0;before<=200;before+=5){
    const f={w:1200,h:800,length:200,corner,from:({topRight:"top",bottomRight:"right",bottomLeft:"bottom",topLeft:"left"})[corner],before,after:200-before,thin:1};
    const parts=Carry.parts(f,p);assert.ok(parts.length>=1&&parts.length<=2);
    for(const part of parts){assert.ok(Number.isFinite(part.x)&&Number.isFinite(part.y));assert.doesNotMatch(part.path,/NaN|Infinity/);assert.ok(part.path.endsWith('Z'));}
    assert.ok(Carry.goo(f,p)>=0&&Carry.goo(f,p)<=70*.28);
  }
});
test('terminal cleanup is exactly once, including promise and event reordering',()=>{
  const events=[],l=new Carry.Lifecycle(e=>events.push(e));const id=l.begin();assert.ok(id);
  assert.equal(l.begin(),null);assert.equal(l.release(),id);assert.equal(l.phase,'settling');
  assert.equal(l.finish({id:id+1}),false);assert.ok(l.active());
  assert.equal(l.finish({id,status:'finished'}),true);assert.equal(l.finish({id,status:'cancelled'}),false);
  assert.equal(l.present(id),false);assert.equal(events.length,1);
  const next=l.begin();assert.notEqual(next,id);assert.equal(l.finish({id}),false);assert.equal(l.present(next),true);
  assert.equal(l.finish({id:next,status:'cancelled'}),true);assert.equal(events.length,2);
});
test('gesture generations remain monotonic across a backwards clock adjustment',()=>{
 const now=Date.now;try{let time=1000000;Date.now=()=>time;const l=new Carry.Lifecycle(()=>{}),first=l.begin();l.finish({id:first});time-=60000;const second=l.begin();assert.ok(second>first);assert.equal(l.finish({id:first}),false);assert.ok(l.active());}finally{Date.now=now;}
});

// The shipped overlay script, run with inert IPC/DOM stand-ins (no browser, no native paint).
// It establishes only the page's reporting contract: the ACK still follows two animation frames,
// a cell that cannot load is reported with its stage instead of silently never acknowledging,
// and a decode() rejection after a sized load is reported without blocking the ACK.
function runOverlay({imageError=false,decodeError=null}={}){
  const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm');
  const html=fs.readFileSync(path.join(__dirname,'../matra/ui/dropzones.html'),'utf8');
  const script=html.match(/<script>([\s\S]*?)<\/script>/)[1];
  const calls=[],frames=[],el=()=>({attrs:{},children:[],setAttribute(k,v){this.attrs[k]=v;},replaceChildren(...c){this.children=c;}});
  const nodes={stage:el(),body:el(),cells:el(),'body-clip':el(),grip:el(),blur:el()};
  class FakeImage{constructor(){this.naturalWidth=0;this.naturalHeight=0;}set src(v){queueMicrotask(()=>{if(imageError)this.onerror();else{this.naturalWidth=40;this.naturalHeight=40;this.onload();}});}decode(){return decodeError?Promise.reject(Error(decodeError)):Promise.resolve();}}
  const p={layouts:Carry.edges.map(()=>({depth:70})),cells:[{image:'data:image/svg+xml;charset=utf-8,%3Csvg%2F%3E',width:40,height:40}],source_edge:'left',fill:'#000',stroke:'#111',ink:'#fff',flare:38.7,corner_radius:29.6,bleed:2,scale:1};
  const frame={id:42,w:1200,h:800,place:2300,length:200,corner:null,before:200,after:0,from:'left',to:'left',thin:1,back:0,rings:[[30,400]],settling:false};
  const window={__TAURI__:{core:{invoke:async(name,args)=>{calls.push({name,args});return name==='get_zones'?{presentation:p,frame}:null;}},event:{listen:async()=>()=>{}}},
    MatraCarry:Carry,MatraNotchReference:Notch,addEventListener(){}};
  const context={window,document:{getElementById:id=>nodes[id],createElementNS:()=>el(),visibilityState:'visible'},Image:FakeImage,
    requestAnimationFrame:f=>frames.push(f),setTimeout,clearTimeout,innerWidth:1200,innerHeight:800,devicePixelRatio:1,MatraCarry:Carry,MatraNotchReference:Notch,Promise,Object,Error,String};
  vm.runInNewContext(script,context);
  return {calls,frames,nodes};
}
const settle=()=>new Promise(r=>setTimeout(r,20));
test('overlay acknowledges only after two animation frames and reports its stages',async()=>{
  const o=runOverlay();await settle();
  const stages=o.calls.filter(c=>c.name==='carry_overlay_report').map(c=>c.args.stage);
  assert.deepEqual([...stages],['script','zones','cells','drawn']);
  assert.equal(o.calls.some(c=>c.name==='carry_overlay_ready'),false);
  o.frames.shift()();assert.equal(o.calls.some(c=>c.name==='carry_overlay_ready'),false);
  o.frames.shift()();await settle();
  assert.deepEqual(o.calls.filter(c=>c.name==='carry_overlay_ready').map(c=>c.args.id),[42]);
  assert.equal(o.nodes.cells.children.length,1);assert.equal(o.nodes.grip.children.length,6);
  assert.ok(o.calls.every(c=>c.name!=='carry_overlay_report'||!c.args.error));
});
test('a cell that cannot load is reported with its stage and never acknowledged',async()=>{
  const o=runOverlay({imageError:true});await settle();
  const failure=o.calls.find(c=>c.name==='carry_overlay_report'&&c.args.error);
  assert.equal(failure.args.id,42);assert.equal(failure.args.stage,'zones');assert.match(failure.args.error,/cell 1 image failed to load/);
  assert.equal(o.frames.length,0);assert.equal(o.calls.some(c=>c.name==='carry_overlay_ready'),false);
});
test('a decode() rejection after a sized load is reported but the frame ACK still decides',async()=>{
  const o=runOverlay({decodeError:'Invalid image type.'});await settle();
  const cells=o.calls.find(c=>c.name==='carry_overlay_report'&&c.args.stage==='cells');
  assert.equal(cells.args.detail,'cell 1 decode: Invalid image type.');assert.equal(cells.args.error,null);
  o.frames.shift()();o.frames.shift()();await settle();
  assert.ok(o.calls.some(c=>c.name==='carry_overlay_ready'));
});
