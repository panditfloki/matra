const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const root = path.join(__dirname, '..');
const Sources = require('../matra/ui/matra-providers.js');
const Dashboard = require('../matra/ui/dashboard.js');
const Parity = require('../matra/ui/matra-parity.js');
require('../matra/ui/matra-activity.js');
require('../matra/ui/matra-parity-card.js');
const window = (id,used=.25) => ({id,label:id,used});

test('seven fixed sources retain provider-specific headline and secondary windows',()=>{
  const cases = [
    ['minimax',[window('session'),window('weekly',.8)],'session','weekly'],
    ['devin',[window('daily'),window('weekly',.9)],'daily','weekly'],
    ['devin',[{...window('overage'),count:0}],'overage',null],
    ['kiro',[window('credits'),window('bonus',.9)],'credits',null],
    ['kiro',[window('bonus',.9)],null,null],
    ['amp',[window('agent'),window('orb',.9)],'agent',null],
    ['amp',[window('free')],'free',null],
    ['apify',[window('monthly')],'monthly',null],
    ['kilo',[window('opaque-short'),window('opaque-long',.9),{...window('balance'),count:0}],'opaque-short','opaque-long'],
    ['gemini-api',[{...window('month'),count:120},{...window('today'),count:20}],'month',null],
  ];
  for(const [id,windows,head,week] of cases) {
    assert.equal(Dashboard.headlineOf({windows},id)?.id ?? null,head,id);
    assert.equal(Dashboard.weeklyOf({windows},id)?.id ?? null,week,id);
  }
});

test('a measured monetary zero displays its currency and an absent quota never becomes zero percent',()=>{
  for(const id of ['apify','kilo','devin']) {
    const w={id:'balance',label:'Credit balance',count:0,used:0};
    const source={id,base:id,name:id,snap:{status:'ok',kind:'usage',windows:[w],details:{texts:{balance:{used_text:'$0.00'}}}}};
    const html=Dashboard.windowHtml(w,source,{now:0});
    assert.match(html,/\$0\.00/);assert.doesNotMatch(html,/0%|role="progressbar"/);
  }
  assert.equal(Dashboard.headlineOf({windows:[]},'kiro'),null);
  assert.equal(Dashboard.headlineOf({windows:[]},'gemini-api'),null);
  const w={id:'month',label:'Tokens this month',count:120,used:0,derived:true};
  const html=Dashboard.windowHtml(w,{snap:{kind:'usage'}},{now:0});
  assert.match(html,/120/);assert.doesNotMatch(html,/0%|request today/);
});

test('MiniMax regions and auth modes are explicit and Gemini budget is personal',()=>{
  const mini=Sources.sourceHtml({id:'minimax',name:'MiniMax',options:{region:'china',auth:'cookie'},secret_label:'MiniMax console Cookie header',accepts_secret:true});
  assert.match(mini,/value="china" selected/);assert.match(mini,/value="cookie" selected/);assert.match(mini,/name="secret" value=""/);
  const gemini=Sources.sourceHtml({id:'gemini-api',name:'Gemini API',options:{monthly_token_budget:250000},accepts_secret:false});
  assert.match(gemini,/Personal monthly token budget/);assert.match(gemini,/value="250000"/);assert.doesNotMatch(gemini,/name="secret"/);
  for(const id of ['amp','devin','kiro','kilo']) assert.doesNotMatch(Sources.sourceHtml({id,name:id,accepts_secret:false}),/name="secret"/);
});

async function chooseOptions(id,values,previous={region:'international',auth:'key',monthly_token_budget:null}) {
  const calls=[],handlers={};
  const host={innerHTML:'',querySelectorAll:()=>[],addEventListener:(name,fn)=>{handlers[name]=fn;}};
  const view=Sources.mount(host,{invoke:async(command,args)=>{calls.push({command,args});return command==='get_provider_registry'?[{id,name:id,options:previous}]:command==='get_parity_usage'?{}:command==='get_opencode_bridge_status'?{}:[];}});
  view.state.registry=[{id,name:id,options:previous}];
  const source={dataset:{source:id},querySelector:selector=>{const name=selector.match(/name="([^"]+)"/)[1];return name in values ? {value:values[name]} : null;}};
  const button={dataset:{sourceAction:'options'},closest:selector=>selector==='[data-source]'?source:null};
  handlers.click({target:{closest:()=>button}});
  await new Promise(resolve=>setImmediate(resolve));
  return {calls,host,view};
}

test('options controls invoke the real settings command and invalid budgets do not persist',async()=>{
  const mini=await chooseOptions('minimax',{region:'china',auth:'cookie'});
  assert.deepEqual(mini.calls.find(c=>c.command==='set_fixed_provider_options').args,{id:'minimax',options:{region:'china',auth:'cookie',monthly_token_budget:null}});
  const gemini=await chooseOptions('gemini-api',{monthly_token_budget:'250000'});
  assert.equal(gemini.calls.find(c=>c.command==='set_fixed_provider_options').args.options.monthly_token_budget,250000);
  for(const bad of ['-1','Infinity','1.5','1000000000000001']) {
    const result=await chooseOptions('gemini-api',{monthly_token_budget:bad});
    assert.ok(!result.calls.some(c=>c.command==='set_fixed_provider_options'));assert.match(result.host.innerHTML,/whole token budget/);
  }
});

test('exact source clocks override guesses and credential text cannot escape details',()=>{
  const w={...window('opaque'),'duration_ms':17*3600000,resets_at:17*3600000};
  assert.equal(Parity.durationMs(w),17*3600000);
  const source={snap:{kind:'usage',details:{texts:{opaque:{detail:'<script>fixture</script>'}}}}};
  const html=Dashboard.windowHtml(w,source,{now:0,parity:{}});
  assert.match(html,/&lt;script&gt;fixture&lt;\/script&gt;/);assert.doesNotMatch(html,/<script>/);
});

test('provider artwork reuses native assets and Gemini API keeps its own spark',()=>{
  for(const id of ['minimax','amp','apify','kilo']) {
    assert.deepEqual(fs.readFileSync(path.join(root,`matra/glyphs/${id}.svg`)),fs.readFileSync(path.join(root,`macos/Sources/Assets.xcassets/glyph-${id}.imageset/${id}.svg`)));
  }
  assert.deepEqual(fs.readFileSync(path.join(root,'matra/glyphs/devin.png')),fs.readFileSync(path.join(root,'macos/Sources/Assets.xcassets/glyph-devin.imageset/devin.png')));
  assert.deepEqual(fs.readFileSync(path.join(root,'matra/glyphs/gemini-api.svg')),fs.readFileSync(path.join(root,'matra/glyphs/gemini-alt.svg')));
  assert.notDeepEqual(fs.readFileSync(path.join(root,'matra/glyphs/gemini-api.svg')),fs.readFileSync(path.join(root,'matra/glyphs/gemini.svg')));
  assert.match(fs.readFileSync(path.join(root,'matra/glyphs/kiro.svg'),'utf8'),/fill-rule="evenodd"/);
  const html=Dashboard.glyphHtml({base:'devin',letter:'D'},{devin:{kind:'template',url:'data:image/png;base64,AAAA'}});
  assert.match(html,/mask-image/);assert.doesNotMatch(html,/<img/);
});

test('notch inline script still parses with fixed selectors and template artwork',()=>{
  const html=fs.readFileSync(path.join(root,'matra/ui/notch.html'),'utf8');
  for(const [,script] of html.matchAll(/<script(?:\s[^>]*)?>([\s\S]*?)<\/script>/g)) if(script.trim()) new vm.Script(script);
});
