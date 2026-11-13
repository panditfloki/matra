#!/usr/bin/env node
// Explicit native platform entries. No implicit install, production release or host profile write.
import {readFileSync,writeFileSync,mkdirSync,readdirSync,copyFileSync,existsSync,realpathSync,statSync} from 'node:fs';
import {resolve,dirname,join,relative,isAbsolute} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..'),args=process.argv.slice(2);
const value=key=>{const i=args.indexOf(key);return i<0?null:args[i+1];};
const modes=['--check','--test','--bundle'].filter(x=>args.includes(x));
const os=value('--platform')||({darwin:'macos',win32:'windows',linux:'linux'}[process.platform]);
if(modes.length!==1||!['macos','windows','linux'].includes(os)) throw new Error('Usage: --check|--test|--bundle [--platform macos|windows|linux] [--offline] [--tauri-cli PATH] [--workspace ABS --xcodegen ABS --packages ABS --arch arm64|x86_64]');
if(os!==({darwin:'macos',win32:'windows',linux:'linux'}[process.platform])) throw new Error('Run this entry on its native operating system.');
function real(path){let p=resolve(path),parts=[];while(!existsSync(p)){parts.unshift(p.slice(dirname(p).length+1));p=dirname(p);}return join(realpathSync(p),...parts);}
const work=real(process.env.MATRA_BUILD_ROOT||join(root,'.build/development'));
// Explicit roots must remain inside this checkout. Do not inherit global cargo/temp/cache destinations.
const inside=relative(real(root),work);if(inside==='..'||inside.startsWith('../')||inside.startsWith('..\\')||isAbsolute(inside))throw new Error('MATRA_BUILD_ROOT must stay inside this source checkout.');
const owned=path=>{const p=real(path),r=relative(real(root),p);if(r==='..'||r.startsWith('../')||r.startsWith('..\\')||isAbsolute(r))throw new Error('Build path resolves outside this checkout.');return p;};
const env={...process.env,CARGO_HOME:owned(join(work,'cargo')),CARGO_TARGET_DIR:owned(join(work,'target')),TMPDIR:owned(join(work,'tmp')),MATRA_TEST_WORKSPACE:owned(join(work,'tests')),XDG_CACHE_HOME:owned(join(work,'cache')),npm_config_cache:owned(join(work,'npm'))};
for(const k of ['CARGO_HOME','CARGO_TARGET_DIR','TMPDIR','MATRA_TEST_WORKSPACE','XDG_CACHE_HOME','npm_config_cache'])mkdirSync(env[k],{recursive:true});
env.TMP=env.TMPDIR;env.TEMP=env.TMPDIR;delete env.MATRA_PREVIEW;delete env.MATRA_DATA_DIR;
const run=(cmd,argv,cwd=root,extra={})=>{const r=spawnSync(cmd,argv,{cwd,env:{...env,...extra},stdio:'inherit',shell:false});if(r.error)throw r.error;if(r.status!==0)throw new Error(`${cmd} failed (${r.status})`);};
if(os==='macos'){
  const script=join(root,'native/macos/Scripts/build-development.py');
  if(modes[0]==='--check'){run('/usr/bin/python3',['-B',script,'--help']);process.exit(0);}
  const native=[];for(const flag of ['--workspace','--xcodegen','--packages']){if(!value(flag))throw new Error(`Native Swift build requires ${flag}`);native.push(flag,value(flag));}
  if(value('--arch'))native.push('--arch',value('--arch'));
  run('/usr/bin/python3',['-B',script,...native,...(modes[0]==='--test'?['--test-only']:['--tests','--package'])]);process.exit(0);
}
if(process.arch!=='x64')throw new Error('Windows and Linux development packaging currently targets x64.');
run(process.execPath,[join(root,'scripts/verify-development-inputs.mjs')]);
const cargo=process.platform==='win32'?'cargo.exe':'cargo';
run(cargo,['+1.97.1','--version']);
const common=['--locked','--manifest-path',join(root,'native/Cargo.toml'),...(args.includes('--offline')?['--offline']:[])];
run(cargo,['+1.97.1','build','--release',...common,'-p','matra-hook','--target-dir',owned(join(root,'native/target/hook'))],root,os==='windows'?{RUSTFLAGS:'-C target-feature=+crt-static'}:{});
const checks=['check-ui-scripts.mjs','test-claude-auth-ui.cjs','test-ko-i18n.cjs','test-matra-interactions.cjs','test-matra-appearance.cjs','test-matra-copy.cjs','test-matra-quota.cjs','test-parity-ui.cjs','test-mac-notch.cjs','test-platform-capabilities.cjs','test-fixed-providers.cjs','test-carry.cjs','test-presentation.cjs'];
for(const name of checks)run(process.execPath,[join(root,'native/scripts',name)]);
for(const name of ['test-codex-details.cjs','test-codex-headline.cjs'])run(process.execPath,[join(root,'native',name)]);
run(cargo,['+1.97.1',modes[0]==='--check'?'check':'test',...common,'--workspace']);
if(modes[0]!=='--bundle')process.exit(0);
const cli=value('--tauri-cli')||process.env.MATRA_TAURI_CLI;if(!cli)throw new Error('Supply @tauri-apps/cli 2.11.4 via --tauri-cli.');
const metadata=JSON.parse(readFileSync(join(dirname(resolve(cli)),'package.json')));if(metadata.name!=='@tauri-apps/cli'||metadata.version!=='2.11.4')throw new Error('Packaging CLI must be pinned to 2.11.4.');
run(process.execPath,[join(root,'scripts/prepare-development-bundler.mjs'),'--platform',os,...(args.includes('--offline')?['--offline']:[])]);
const started=Date.now();
run(process.execPath,[resolve(cli),'build','--bundles',os==='windows'?'nsis':'deb,appimage','--','--locked',...(args.includes('--offline')?['--offline']:[])],join(root,'native/matra'),{RUSTUP_TOOLCHAIN:'1.97.1',...(os==='linux'?{LDAI_RUNTIME_FILE:owned(join(env.CARGO_TARGET_DIR,'.tauri/runtime-x86_64'))}:{})});
const out=owned(join(root,'artifacts/development',`${os}-x64`));mkdirSync(out,{recursive:true});
const packages=[];
function collect(folder){if(!existsSync(folder))return;for(const e of readdirSync(folder,{withFileTypes:true})){const p=join(folder,e.name);if(e.isDirectory())collect(p);else if(/0\.2\.0.*(setup\.exe|\.deb|\.AppImage)$/.test(e.name)&&statSync(p).mtimeMs>=started-2000){copyFileSync(p,join(out,e.name));packages.push(e.name);}}}
collect(join(env.CARGO_TARGET_DIR,'release/bundle'));
if(packages.length!==(os==='windows'?1:2))throw new Error(`Expected new ${os} packages, found ${packages.length}; stale files are excluded.`);
const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
writeFileSync(join(out,'SHA256SUMS.txt'),packages.map(n=>`${hash(join(out,n))}  ${n}`).join('\n')+'\n');
writeFileSync(join(out,'build-receipt.json'),JSON.stringify({version:'0.2.0',channel:'development',identifier:'com.dydxfx.matra.development020',os,arch:process.arch,rust:'1.97.1',tauri_cli:'2.11.4',cargo_lock_sha256:hash(join(root,'native/Cargo.lock')),packages:packages.map(name=>({name,sha256:hash(join(out,name))})),native_acceptance:'pending CI lifecycle and target-device review'},null,2)+'\n');
console.log(`Development packages: ${out}`);
