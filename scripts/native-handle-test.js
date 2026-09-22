const {execFileSync}=require('node:child_process');
const path=require('node:path'),assert=require('node:assert/strict');
function run(js){return JSON.parse(execFileSync(process.execPath,[path.join(__dirname,'native-inspect.js'),'notch',js],{encoding:'utf8'})).value;}
const original=run("invoke('get_notch_edge')");
const results=[];
try{
 for(const edge of ['left','right','top','bottom']){
  run(`invoke('set_notch_edge',{edge:'${edge}'}).then(()=>new Promise(r=>setTimeout(r,650)))`);
  const result=run(`unfold();setDockActive(true);placeHandles();(()=>{const p=pill.getBoundingClientRect(),v=edgeIsVertical(),x=p.left+p.width/2,y=p.top+p.height/2;return {center:handleIntent(x,y),near:handleIntent(v?x:p.left+1,v?p.top+1:y),far:handleIntent(v?x:p.right-1,v?p.bottom-1:y),move:handleIntent(moveAt.x,moveAt.y),orb:handleIntent(orbAt.x,orbAt.y),bridge:handleIntent(v?x:p.right+1,v?p.bottom+1:y)}})()`);
  assert.deepEqual(result,{center:null,near:'move',far:'orb',move:'move',orb:'orb',bridge:'orb'});
  // Disable transitions during CSS contract checks; native pointer events remain active.
  const state=run(`(()=>{orb.style.transition='none';moveHandle.style.transition='none';const read=()=>[getComputedStyle(moveHandle).opacity,getComputedStyle(orb).opacity];setHovered(null);const idle=read();setHovered('move');const move=read();setHovered('orb');const settings=read();setHovered(null);orb.style.transition='';moveHandle.style.transition='';return {idle,move,settings}})()`);
  assert.deepEqual(state,{idle:['0','0'],move:['1','0'],settings:['0','1']});
  results.push({edge,...result,...state});
 }
 console.log(JSON.stringify({passed:true,results},null,2));
}finally{run(`invoke('set_notch_edge',{edge:${JSON.stringify(original)}})`);}
