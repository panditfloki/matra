const {execFileSync}=require('node:child_process');
const path=require('node:path');
const assert=require('node:assert/strict');
function run(js,shot){return JSON.parse(execFileSync(process.execPath,[path.join(__dirname,'native-inspect.js'),'notch',js,...(shot?[shot]:[])],{encoding:'utf8'})).value;}
const tones=run('[0,.12,.17,.499,.5,.749,.75,.899,.9,1,1.1,NaN,-1].map(tone)');
assert.deepEqual(tones,['green','green','green','green','amber','amber','orange','orange','red','red','red'].map(x=>`var(--quota-${x})`).concat(['var(--matra-muted)','var(--matra-muted)']));
assert.deepEqual(run('[{used:0},{used:.12},{used:.8,count:10},{used:null},{used:NaN},{used:-1},null].map(metered)'),[true,true,false,false,false,false,false]);
const sample=run(`(()=>{const oldProviders=providers,oldWeekly=weeklyRing;try{weeklyRing='outside';providers=()=>[{id:'claude',base:'claude',name:'Claude',glyph:'C',snap:{status:'ok',fetched_at:Date.now(),windows:[{id:'session',used:.12},{id:'seven_day',used:.82}]}}];renderRing();const main=document.querySelector('.quota-current'),week=document.querySelector('.quota-weekly');return {main:main.getAttribute('stroke'),week:week.getAttribute('stroke'),mainWidth:main.getAttribute('stroke-width'),weekWidth:week.getAttribute('stroke-width'),label:document.querySelector('.pct').textContent};}finally{providers=oldProviders;weeklyRing=oldWeekly;renderRing();}})()`);
assert.equal(sample.main,'var(--quota-green)');assert.equal(sample.week,'var(--quota-orange)');assert.equal(sample.label,'12%');assert.equal(sample.mainWidth,'5.1');assert.equal(sample.weekWidth,'3.2');
console.log(JSON.stringify({passed:true,thresholds:tones.length,sample},null,2));
