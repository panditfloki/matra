#!/usr/bin/env node
// Seed Tauri 2.11.4's local tool directory before it can use moving download URLs.
import {readFileSync,writeFileSync,mkdirSync,existsSync,realpathSync,copyFileSync,chmodSync,renameSync,rmSync} from 'node:fs';
import {dirname,resolve,join,relative,isAbsolute,basename} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {execFileSync} from 'node:child_process';
const root=realpathSync(resolve(dirname(fileURLToPath(import.meta.url)),'..'));
const args=process.argv.slice(2),platform=args[args.indexOf('--platform')+1],offline=args.includes('--offline');
if(!['windows','linux'].includes(platform))throw new Error('Expected --platform windows|linux');
function contained(path){
  const result=resolve(path),r=relative(root,result);
  if(r==='..'||r.startsWith('../')||r.startsWith('..\\')||isAbsolute(r))throw new Error('Bundler tools must remain in the checkout');
  let cursor=result;
  while(!existsSync(cursor))cursor=dirname(cursor);
  const real=relative(root,realpathSync(cursor));
  if(real==='..'||real.startsWith('../')||real.startsWith('..\\')||isAbsolute(real))throw new Error('Bundler tool symlink leaves checkout');
  return result;
}
const target=contained(process.env.CARGO_TARGET_DIR||join(root,'.build/development/target'));
const cache=contained(join(root,'.build/development/bundler-inputs'));
const tools=contained(join(target,'.tauri'));
for(const folder of [cache,tools])mkdirSync(folder,{recursive:true});
const manifest=JSON.parse(readFileSync(join(root,'build/development/bundler-inputs.json')));
const inputs=manifest.inputs.filter(item=>item.platform===platform);
const sha256=data=>createHash('sha256').update(data).digest('hex');
for(const input of inputs){
  if(basename(input.file)!==input.file||!/^[a-f0-9]{64}$/.test(input.sha256))throw new Error('Invalid pinned bundler input');
  if(!Number.isFinite(Date.parse(input.published))||(Date.now()-Date.parse(input.published))/86400000<14)throw new Error(`Bundler input is younger than 14 days: ${input.file}`);
  const path=contained(join(cache,input.file));
  if(!existsSync(path)||sha256(readFileSync(path))!==input.sha256){
    if(offline)throw new Error(`Missing verified offline bundler input: ${input.file}`);
    const response=await fetch(input.url,{signal:AbortSignal.timeout(120000)});
    if(!response.ok)throw new Error(`Bundler download failed ${response.status}: ${input.file}`);
    const data=Buffer.from(await response.arrayBuffer());
    if(sha256(data)!==input.sha256)throw new Error(`Bundler input hash changed: ${input.file}; refusing moving replacement`);
    writeFileSync(path,data);
  }
}
if(platform==='linux'){
  for(const input of inputs){
    const dest=contained(join(tools,input.file));
    // Tauri clears bytes 8..10 of linuxdeploy after validation. Restore a fresh,
    // verified copy for each package; immutable originals stay in the cache.
    copyFileSync(join(cache,input.file),dest);chmodSync(dest,0o755);
  }
}else{
  if(process.platform!=='win32')throw new Error('NSIS extraction requires native Windows');
  const archive=join(cache,'nsis-3.11.zip');
  const entries=execFileSync('tar.exe',['-tf',archive],{encoding:'utf8'}).trim().split(/\r?\n/);
  if(entries.some(entry=>!entry.startsWith('nsis-3.11/')||entry.split(/[\\/]/).includes('..')))throw new Error('Unsafe NSIS archive paths');
  const nsis=contained(join(tools,'NSIS')),extracted=contained(join(tools,'nsis-3.11'));
  // Generated tools only, never an installed NSIS or a profile directory.
  rmSync(nsis,{recursive:true,force:true});rmSync(extracted,{recursive:true,force:true});
  execFileSync('tar.exe',['-xf',archive,'-C',tools],{stdio:'inherit'});
  renameSync(extracted,nsis);
  const plugins=contained(join(nsis,'Plugins/x86-unicode/additional'));mkdirSync(plugins,{recursive:true});
  copyFileSync(join(cache,'nsis_tauri_utils.dll'),join(plugins,'nsis_tauri_utils.dll'));
}
const receipt=contained(join(root,'artifacts/development',`${platform}-x64`,'bundler-input-receipt.json'));
mkdirSync(dirname(receipt),{recursive:true});writeFileSync(receipt,JSON.stringify({tauri_cli:manifest.tauri_cli,inputs},null,2)+'\n');
console.log(`PASS: ${inputs.length} age-eligible, SHA-256 pinned ${platform} bundler inputs staged in the Cargo target directory.`);
