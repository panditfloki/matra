// Read only the debugger endpoints belonging to the disposable fixture app.
const {execFileSync}=require('node:child_process');
const {existsSync,readFileSync,writeFileSync,mkdirSync}=require('node:fs');
const path=require('node:path');
const cache=path.resolve(__dirname,'../.build/development-lifecycle/native-debug-ports.json');
async function probe(ports){
  const groups=await Promise.all(ports.map(async port=>{
    try {
      const response=await fetch(`http://127.0.0.1:${port}/json/list`,{signal:AbortSignal.timeout(1500)});
      if(!response.ok)return [];
      const targets=await response.json();
      return Array.isArray(targets)?targets.filter(t=>typeof t.webSocketDebuggerUrl==='string'):[];
    } catch {return [];}
  }));
  return groups.flat();
}
async function listNativeTargets({page,refresh=false}={}){
  const app=Number(process.env.MATRA_NATIVE_APP_PID);
  if(!app)return probe([9337]); // existing manual inspection entry
  if(process.platform!=='win32'||process.env.GITHUB_ACTIONS!=='true'||!Number.isSafeInteger(app)||app<=0)throw new Error('Dynamic native debugger discovery requires a disposable Windows fixture');
  let saved={};try{if(existsSync(cache))saved=JSON.parse(readFileSync(cache));}catch{}
  const ports=saved.app===app&&Array.isArray(saved.ports)?saved.ports:[];
  let targets=await probe(ports);
  if(refresh||!targets.length||(page&&!targets.some(t=>t.url.endsWith(`/${page}.html`)))){
    const result=execFileSync('powershell.exe',['-NoLogo','-NoProfile','-NonInteractive','-File',path.join(__dirname,'native-debug-ports.ps1'),'-AppProcessId',String(app)],{encoding:'utf8',timeout:15000});
    const found=JSON.parse(result),next=(Array.isArray(found)?found:[found]).filter(p=>Number.isInteger(p)&&p>0&&p<=65535);
    mkdirSync(path.dirname(cache),{recursive:true});writeFileSync(cache,JSON.stringify({app,ports:next}));
    targets=await probe(next);
  }
  if(!targets.some(t=>t.url.endsWith('/notch.html')))throw new Error('The fixture notch debugger is unavailable; absence of another target is not proof of closure');
  return targets;
}
module.exports={listNativeTargets};
