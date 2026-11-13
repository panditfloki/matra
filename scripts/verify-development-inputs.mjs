#!/usr/bin/env node
import {readFileSync} from 'node:fs';
import {resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const report=JSON.parse(readFileSync(resolve(root,'build/development/dependency-age-audit.json')));
const versions=new Map(report.packages.map(p=>[`${p.name}@${p.version}`,p]));
const lock=readFileSync(resolve(root,'native/Cargo.lock'),'utf8');
let count=0;
for(const entry of lock.split('[[package]]').filter(s=>s.includes('source = "registry'))){
  const name=/^name = "([^"]+)"/m.exec(entry)?.[1], version=/^version = "([^"]+)"/m.exec(entry)?.[1];
  const evidence=versions.get(`${name}@${version}`);
  if(!evidence || !evidence.old_enough || (Date.now()-Date.parse(evidence.published))/86400000<14) throw new Error(`No age-eligible receipt for locked dependency ${name}@${version}`);
  count++;
}
const config=JSON.parse(readFileSync(resolve(root,'native/matra/tauri.conf.json')));
if(config.version!=='0.2.0'||config.identifier!=='com.dydxfx.matra.development020'||config.bundle.createUpdaterArtifacts!==false) throw new Error('Development package identity or updater policy drifted.');
console.log(`PASS: ${count} unchanged locked registry dependencies satisfy the 14-day rule; isolated Development 0.2.0 identity.`);

const npmLock=JSON.parse(readFileSync(resolve(root,'build/development/tools/package-lock.json')));
const npmAudit=JSON.parse(readFileSync(resolve(root,'build/development/npm-age-audit.json')));
for(const [key,pkg] of Object.entries(npmLock.packages)){
  if(!key)continue;
  const name=key.replace(/^node_modules\//,'');
  const evidence=npmAudit.packages.find(p=>p.name===name&&p.version===pkg.version&&p.integrity===pkg.integrity);
  if(!evidence||(Date.now()-Date.parse(evidence.published))/86400000<14)throw new Error(`Packaging dependency age/integrity changed: ${name}`);
}
console.log('PASS: pinned packaging CLI and platform binaries match age and integrity receipts.');

const workflow=readFileSync(resolve(root,'.github/workflows/development-platforms.yml'),'utf8');
const actionAudit=JSON.parse(readFileSync(resolve(root,'build/development/action-age-audit.json')));
for(const match of workflow.matchAll(/uses: (actions\/[a-z-]+)(?:\/[a-z-]+)?@([a-f0-9]{40})/g)){
  const evidence=actionAudit.actions.find(a=>a.repository===match[1]&&a.commit===match[2]);
  if(!evidence||(Date.now()-Date.parse(evidence.committed_at))/86400000<14)throw new Error(`Unverified action: ${match[0]}`);
}
const bundler=JSON.parse(readFileSync(resolve(root,'build/development/bundler-inputs.json')));
for(const input of bundler.inputs){
  if(!/^[a-f0-9]{64}$/.test(input.sha256)||!Number.isFinite(Date.parse(input.published))||(Date.now()-Date.parse(input.published))/86400000<14)throw new Error(`Unverified bundler helper: ${input.file}`);
  if(input.commit&&!input.url.includes('/'+input.commit+'/'))throw new Error(`Bundler script URL is not pinned: ${input.file}`);
}
console.log(`PASS: workflow actions and ${bundler.inputs.length} bundler inputs have age-eligible pinned receipts.`);
