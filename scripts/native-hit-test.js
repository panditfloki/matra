const {execFileSync}=require('node:child_process');
const path=require('node:path'),assert=require('node:assert/strict');
function run(js){return JSON.parse(execFileSync(process.execPath,[path.join(__dirname,'native-inspect.js'),'notch',js],{encoding:'utf8'})).value;}
const result=run(`(()=>{unfold();hideCard();setDockActive(true);const oldCall=callq;let last;callq=(name,args)=>{if(name==='set_hot')last=args;return Promise.resolve()};try{setHovered(null);reportHot();const idle=last.rects;setHovered('move');reportHot();const move=last.rects;setHovered('orb');reportHot();const settings=last.rects;return {idle,move,settings};}finally{callq=oldCall;setHovered(null);reportHot();}})()`);
assert.equal(result.idle.length,1,'only the main pill is hot while satellites are hidden');
assert.equal(result.move.length,3,'pill, revealed move, explicit bridge');
assert.equal(result.settings.length,3,'pill, revealed settings, explicit bridge');
for(const rects of [result.move,result.settings]){
 assert.ok(rects[2][2]>=0&&rects[2][3]>=0);
 assert.ok(Math.min(rects[2][2],rects[2][3])<=4,'bridge must be only the small gap');
}
console.log(JSON.stringify({passed:true,rectCounts:[result.idle.length,result.move.length,result.settings.length]},null,2));
