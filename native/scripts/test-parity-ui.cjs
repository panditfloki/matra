const {test} = require('node:test');
const assert = require('node:assert/strict');
const {readFileSync,existsSync} = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const ui = path.join(__dirname,'../matra/ui');
const P = require('../matra/ui/matra-parity.js');
const A = require('../matra/ui/matra-activity.js');
const Sources = require('../matra/ui/matra-providers.js');
const Card = require('../matra/ui/matra-parity-card.js');
const week=7*86400000;
const cell=(id,used,left)=>({id,snap:{status:'ok',windows:[{id:'weekly',label:'Weekly',used,resets_at:left}]}});
const adapter={headline:c=>c.snap.windows[0],weekly:c=>c.snap.windows[1],fresh:c=>c.snap.status==='ok'};

test('expiring quota, remaining quota, stale and unknown accounts order differently',()=>{
  const expiring=cell('expiring',.3,23*3600000),fresh=cell('fresh',0,6*86400000);
  const stale=cell('stale',0,week);stale.snap.status='stale';
  assert.deepEqual(P.order([stale,fresh,expiring],'burnDeficit',adapter,0).map(c=>c.id),['expiring','fresh','stale']);
  assert.deepEqual(P.order([expiring,fresh],'remaining',adapter,0).map(c=>c.id),['fresh','expiring']);
  assert.deepEqual(P.order([fresh,expiring],'manual',adapter,0).map(c=>c.id),['fresh','expiring']);
  assert.equal(P.burnPoints({id:'unknown',used:.1,resets_at:60000},0),null);
});

test('an exhausted weekly limit shuts the account; unknown usage never does',()=>{
  assert.equal(P.isShut({shut_rings_when_spent:true},{used:.1},{used:1}),true);
  assert.equal(P.isShut({shut_rings_when_spent:true},{used:NaN},{count:50,used:1}),false);
  const spent=cell('spent',1,week);
  assert.equal(P.present([spent],{hide_depleted:true},adapter,0).length,1,'The last ring remains recoverable');
});

test('the expanded grip accepts a direct left press without hover or a modifier',()=>{
  assert.equal(P.gripPress({folded:false,showMove:true,button:0,onGrip:true}),true);
  assert.equal(P.gripPress({folded:true,showMove:true,button:0,onGrip:true}),false);
  assert.equal(P.gripPress({folded:false,showMove:false,button:0,onGrip:true}),false);
  assert.equal(P.gripPress({folded:false,showMove:true,button:2,onGrip:true}),false);
});

test('turning a display choice on and off preserves unrelated selections',()=>{
  const base=P.normalize({shows_remaining:true,tray_providers:['codex']});
  const on=P.merge(base,{shows_limits_in_tray:true});
  const off=P.merge(on,{shows_limits_in_tray:false});
  assert.deepEqual(off,base);
  assert.deepEqual(P.holdOrder([{id:'b'},{id:'a'}],['a','b'],true).map(c=>c.id),['a','b']);
});

test('unknown prices, zero prices and partial totals remain distinguishable',()=>{
  assert.equal(A.money(null,'unavailable'),'Unavailable');
  assert.equal(A.money(0,'complete'),'~₹0.00');
  assert.equal(A.money(0,'none'),'-');
  assert.match(A.money(5,'at_least'),/^At least ₹5/);
  assert.equal(A.priceEntry({model:'m',input:'1',output:'2'}).cache_read,undefined);
  for(const bad of ['','0','-1','Infinity','100001'])assert.equal(A.parseRate(bad),null);
});

function activityHost(respond) {
  const calls=[],handlers={};
  const host={innerHTML:'',closest:()=>null,querySelector:()=>null,addEventListener(n,f){handlers[n]=f;}};
  const view=A.mount(host,{invoke:async(command,args)=>{calls.push({command,args});return respond(command,args);}});
  return {host,view,calls,handlers};
}
const settings={usd_to_inr:84,currency:'INR',custom_prices:[],accounts:{}};
test('activity shows empty and failed reads instead of invented rows',async()=>{
  const empty=activityHost(async c=>c==='get_cost_settings'?settings:c==='get_cost_plans'?[]:{rows:[],accounts:[],projects:[],note:''});
  await empty.view.show();
  assert.match(empty.host.innerHTML,/No Claude or Codex activity/);
  assert.deepEqual(empty.calls.find(c=>c.command==='get_activity_report').args,{range:'week',account:null,project:null,date:null});
  const failed=activityHost(async c=>{if(c==='get_activity_report')throw new Error('read failed');return c==='get_cost_settings'?settings:[];});
  await failed.view.show();assert.match(failed.host.innerHTML,/read failed/);assert.match(failed.host.innerHTML,/Try again/);
});

test('endpoint drafts preserve hidden settings, reject invalid budgets and never echo saved keys',()=>{
  const previous={id:'saved',has_key:true,usage_history:[{day:'2026-10-31',total_tokens:20}]};
  const draft=Sources.draftFrom({name:' Work ',base_url:'https://example.test/v1',monthly_budget_usd:'4'},previous);
  assert.equal(draft.name,'Work');assert.equal(draft.has_key,undefined);assert.deepEqual(draft.usage_history,previous.usage_history);
  assert.equal(Sources.validate(draft),null);
  assert.match(Sources.validate({...draft,monthly_budget_usd:NaN}),/Budgets/);
  assert.match(Sources.validate({...draft,base_url:'https://key@example.test'}),/base URL/);
  assert.equal(Sources.secretArg('',false),null);assert.equal(Sources.secretArg('',true),'');
  const html=Sources.sourceHtml({id:'omlx',name:'oMLX',accepts_secret:true,has_stored_secret:true,endpoint_scope:'network',endpoint:'http://mac.local:8000',guidance:'Runs on a Mac'},{});
  assert.match(html,/type="password" name="secret" value=""/);assert.match(html,/Credential Manager/);
});

test('optional sources render errors and nullable runtime metrics honestly',()=>{
  const html=Card.html({performance:{generation_tokens_per_second:null,queued_requests:0},local_model:{models:[{name:'<script>',memory_bytes:null}]}},'llamacpp');
  assert.doesNotMatch(html,/<script>|NaN|null|tok\/s/);assert.match(html,/0 queued/);
  assert.match(Card.html({details:{runtime_host:'remote'}},'omlx'),/remote Mac/);
});

test('every shipped page has its scripts and styles; every external script parses',()=>{
  for(const name of ['settings.html','notch.html','dropzones.html','activity.html']){
    const html=readFileSync(path.join(ui,name),'utf8');
    for(const [,src] of html.matchAll(/<script[^>]*\bsrc="([^"]+)"/g)){
      assert.ok(existsSync(path.join(ui,src)),`${name}: ${src}`);
      new vm.Script(readFileSync(path.join(ui,src),'utf8'),{filename:src});
    }
    for(const [,src] of html.matchAll(/<link[^>]*href="([^"]+)"/g))assert.ok(existsSync(path.join(ui,src)),`${name}: ${src}`);
  }
});
