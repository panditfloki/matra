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
