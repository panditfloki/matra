// Geometry checks against the installed WebView2. Does not fake native hover events.
const {execFileSync}=require('node:child_process');
const path=require('node:path');
const assert=require('node:assert/strict');
function run(js,shot){return JSON.parse(execFileSync(process.execPath,[path.join(__dirname,'native-inspect.js'),'notch',js,...(shot?[shot]:[])],{encoding:'utf8'})).value;}
const original=run("Promise.all([invoke('get_scale'),invoke('get_notch_edge')])");
const results=[];
try {
  for(const scale of [.8,1,1.25]) for(const edge of ['left','right','top','bottom']) {
    run(`invoke('set_scale',{scale:${scale}}).then(()=>invoke('set_notch_edge',{edge:'${edge}'})).then(()=>new Promise(r=>setTimeout(r,650)))`);
    const value=run(`unfold();placeHandles();(()=>{const p=pill.getBoundingClientRect(),v=edgeIsVertical(),s=getComputedStyle(pill),rest=document.getElementById('rest'),z=parseFloat(document.documentElement.style.zoom)||1;return {edge:notchEdge,scale:${scale},depth:v?pill.offsetWidth:pill.offsetHeight,gap:s.gap,lip:Math.min(rest.offsetWidth,rest.offsetHeight),clip:s.getPropertyValue('--clip-rest'),controls:[orb,moveHandle].filter(e=>e.classList.contains('placed')).map(e=>{const r=e.getBoundingClientRect();return {gap:(v?(e===orb?r.top-p.bottom:p.top-r.bottom):(e===orb?r.left-p.right:p.left-r.right))/z,inside:r.left>=-1&&r.top>=-1&&r.right<=innerWidth+1&&r.bottom<=innerHeight+1}})}})()`,scale===1?`compact-${edge}.png`:undefined);
    assert.equal(value.edge,edge);assert.equal(value.lip,5);assert.equal(value.gap,'10px');assert.ok(value.clip.includes('5px'));
    if(['left','right'].includes(edge))assert.equal(value.depth,62);
    for(const c of value.controls){assert.ok(c.inside);assert.ok(Math.abs(c.gap-2)<.2);}
    results.push(value);
  }
  console.log(JSON.stringify({passed:true,cases:results.length,results},null,2));
} finally {run(`invoke('set_scale',{scale:${original[0]}}).then(()=>invoke('set_notch_edge',{edge:${JSON.stringify(original[1])}}))`);}
