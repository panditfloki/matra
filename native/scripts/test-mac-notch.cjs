const {test}=require('node:test');
const assert=require('node:assert/strict');
const {readFileSync}=require('node:fs');
const path=require('node:path');
const vm=require('node:vm');
const N=require('../matra/ui/matra-notch-reference.js');
const contract=require('../../shared/notch-reference/mac-notch-reference.json');
const L=N.reference.layout;
const html=readFileSync(path.join(__dirname,'../matra/ui/notch.html'),'utf8');
const close=(actual,expected,message)=>assert.ok(Math.abs(actual-expected)<1e-7,`${message||''}: ${actual} != ${expected}`);
const rect=(left,top,width,height)=>({left,top,right:left+width,bottom:top+height,width,height});
const arcPoints=svg=>[...svg.matchAll(/[ML](-?[\d.]+) (-?[\d.]+)/g)].map(m=>({x:Number(m[1]),y:Number(m[2])}));

test('the shipped browser module embeds the generated native-source contract without edits',()=>{
  assert.deepEqual(N.reference,contract);
  assert.match(html,/<script src="matra-notch-reference\.js"><\/script>/);
  for(const call of ['Notch.ringGeometry()','Notch.stackGeometry(','Notch.cardGeometry(','Notch.handleGeometry(','Notch.handleAt(','Notch.settingsArcPath(','Notch.silhouette('])assert.ok(html.includes(call),call);
});

test('ring weights and radii preserve native clearances in the actual 56-unit SVG',()=>{
  const r=N.ringGeometry(),k=L.ringDiameter/r.diameter;
  close((r.trackRadius+r.trackStroke/2)*k,L.ringDiameter/2,'track outer edge');
  close(r.progressStroke*k,L.progressStroke,'thin headline stroke');
  assert.ok(r.progressStroke<r.trackStroke);
  close(r.weeklyOutside*k,L.weeklyOutsideRadius);
  close(r.weeklyInside*k,L.weeklyInsideRadius);
  close(r.activityRadius*k,L.activityDiameter/2);
  assert.ok((r.weeklyOutside+r.weeklyStroke/2)*k<L.sideBodyDepth/2,'outer week clears bezel');
  assert.ok((r.activityRadius-r.activityStroke/2)*k>L.glyphSize/2,'activity clears glyph');
});

test('all edges keep the native cell box when readings are hidden and centre horizontal padding',()=>{
  for(const edge of ['right','left','top','bottom']){
    const g=N.stackGeometry({edge,count:3,lineHeight:17});
    close(g.cellExtent,L.ringDiameter+L.ringLabelGap+17);
    close(g.depth,N.vertical(edge)?L.sideBodyDepth:2*L.sideRingMargin+g.cellExtent);
    close(g.length,g.start+g.end+3*(N.vertical(edge)?g.cellExtent:L.ringDiameter)+2*L.cellSpacing);
    if(N.vertical(edge))assert.notEqual(g.start,g.end);else close(g.start,g.end);
    // No reading-visibility input can remove a line box. The DOM only hides its text.
    assert.deepEqual(N.stackGeometry({edge,count:3,lineHeight:17,showsReadings:false}),g);
  }
});

test('vertical packing spends spacing before overflow and never makes gaps negative',()=>{
  const full=N.stackGeometry({count:6,lineHeight:17});
  const cramped=N.stackGeometry({count:6,lineHeight:17,availableLength:full.length-60});
  close(cramped.length,full.length-60);
  close(cramped.spacing,L.cellSpacing-12);
  const tiny=N.stackGeometry({count:6,lineHeight:17,availableLength:40});
  assert.equal(tiny.spacing,0);assert.ok(tiny.length>40,'caller must retain scrolling for the remaining content');
  assert.ok(Number.isFinite(N.stackGeometry({count:0,lineHeight:17,availableLength:0}).length));
});

test('settings rests in the far flare and the hidden grip keeps its separate discoverable target',()=>{
  for(const edge of ['right','left','top','bottom']){
    const p=N.vertical(edge)?rect(edge==='right'?390:0,90,L.sideBodyDepth,320):rect(90,edge==='top'?0:550,320,98);
    const h=N.handleGeometry(edge,p),v=N.vertical(edge);
    close(v?h.orb.y:h.orb.x,(v?p.bottom:p.right)+L.curlRadius);
    close(v?h.move.y-h.orb.y:h.move.x-h.orb.x,L.orbDiameter/2+L.gripGap+L.gripWidth/2);
    assert.equal(N.handleAt(h.orb,h),'orb');
    assert.equal(N.handleAt(h.move,h),'move');
    assert.equal(N.handleAt(h.move,h,{showMove:false}),null);
    assert.equal(N.handleAt(h.orb,h,{folded:true}),null);
    const intersection={x:h.orb.x+(h.move.x-h.orb.x)*.6,y:h.orb.y+(h.move.y-h.orb.y)*.6};
    assert.ok(Math.hypot(intersection.x-h.move.x,intersection.y-h.move.y)<h.move.reach);
    assert.equal(N.handleAt(intersection,h),'orb','settings wins the overlap');
  }
});

test('the grip has two lines along the edge and three dots across it, at native pitch',()=>{
  for(const edge of ['right','left','top','bottom']){
    const dots=N.gripDots(edge),v=N.vertical(edge);
    assert.equal(dots.length,6);
    assert.equal(new Set(dots.map(p=>v?p.y:p.x)).size,2);
    assert.equal(new Set(dots.map(p=>v?p.x:p.y)).size,3);
    close(dots[0].r,L.gripDot/2);
    close(Math.abs((v?dots[1].x:dots[1].y)-(v?dots[0].x:dots[0].y)),L.gripPitch);
  }
});

test('resting flare samples retain the native walk, trim and fully separated clearance contract',()=>{
  const native=name=>readFileSync(path.join(__dirname,'../macos/Sources',name),'utf8');
  const root=native('Notch/NotchRootView.swift'),settings=native('Features/SettingsHandle.swift');
  const walk=root.slice(root.indexOf('static let flareWalk:'),root.indexOf('/// A cubic from'));
  const flare=settings.slice(settings.indexOf('struct FlareArc:'),settings.indexOf('struct GooArc:'));
  const goo=settings.slice(settings.indexOf('struct GooArc:'),settings.indexOf('struct DiscMerge:'));
  const compact=s=>s.replace(/\/\/[^\n]*/g,'').replace(/\s+/g,'');
  const has=(source,snippet)=>assert.ok(compact(source).includes(compact(snippet)),snippet);
  const configuration=walk.match(/let p: CGFloat = ([\d.]+), steps = (\d+)/);
  assert.ok(configuration,'native flare integration remains explicit');
  assert.equal(Number(configuration[1]),contract.shape.filletRamp);
  assert.equal(Number(configuration[2]),contract.shape.fluidTurnSteps);
  for(const snippet of [
    'let bend = (CGFloat.pi / 2) / (1 - p)',
    'var walk: [(u: CGFloat, v: CGFloat, heading: CGFloat)] = [(0, 0, 0)]',
    'let s = (CGFloat(i) + 0.5) / CGFloat(steps)',
    'let share = s < p ? s / p : (s > 1 - p ? (1 - s) / p : 1)',
    'let before = heading', 'heading += bend * share / CGFloat(steps)',
    'u += cos(heading) / CGFloat(steps)', 'v += sin(heading) / CGFloat(steps)',
    'walk.append((u, v, (before + heading) / 2))',
    'return walk.map { ($0.u / end.u, $0.v / end.v, $0.heading) }'
  ])has(walk,snippet);
  for(const snippet of [
    'let flare = radius + gap',
    'let x = centre.x + flare * (1 - step.v) * bezel.x + flare * step.u * side.x',
    'let y = centre.y + flare * (1 - step.v) * bezel.y + flare * step.u * side.y',
    'let nx = -sin(step.heading) * side.x - cos(step.heading) * bezel.x',
    'let ny = -sin(step.heading) * side.y - cos(step.heading) * bezel.y',
    'let point = CGPoint(x: x + nx * gap, y: y + ny * gap)'
  ])has(flare,snippet);
  has(goo,'let flare = radius + gap');
  has(goo,'let rest = NotchLayout.orbClearance');
  has(goo,'let whole = FlareArc(trim: trim, edge: edge, convex: convex, gap: rest).path(in: square(flare - rest))');
  const trim=settings.slice(settings.indexOf('static func restingTrim(for edge: NotchEdge) ->'),settings.indexOf('private var restingTrim:'));
  for(const [edge,range] of [['right','0.75...1.0'],['left','0.5...0.75'],['top','0.5...0.75'],['bottom','0.25...0.5']])has(trim,`case .${edge}: return ${range}`);
});

// Independent numeric fixtures evaluated from GooNeck.flareWalk + FlareArc in
// the unchanged native source. These are fixed values, never generated by N.
// The final midpoint heading intentionally leaves a 0.0025 pt endpoint offset.
const nativeFlareSamples=[
  {i:0,u:0,v:0,heading:0,x:24.068376068376068,y:0},
  {i:1,u:0.017653521139551816,v:0.000005913421355152505,heading:0.00017044230976507124,x:24.068147224784756,y:-0.6813100752334024},
  {i:24,u:0.421879708817195,v:0.0288894426275823,heading:0.1883387522904037,x:23.208698493953733,y:-13.595528401770029},
  {i:48,u:0.7939513893161899,v:0.21474060783842663,heading:0.7692061439697666,x:19.87959768756543,y:-20.552119504308877},
  {i:72,u:0.9740445882805863,v:0.5851054341195142,heading:1.3660951127670462,x:13.08959762994428,y:-23.369206216734476},
  {i:95,u:1,v:0.9826527184540365,heading:1.5699441152460714,x:0.6594485908165546,y:-24.068381394315587},
  {i:96,u:1,v:1,heading:1.5706258844851315,x:-0.0024998205311179424,y:-24.06837628141366}
];

test('resting settings arc matches independent native samples at all four edges and sizes',()=>{
  const project={right:(x,y)=>[x,y],left:(x,y)=>[-x,y],top:(x,y)=>[y,-x],bottom:(x,y)=>[y,x]};
  for(const edge of Object.keys(project))for(const size of Object.values(contract.sizes)){
    const svg=N.settingsArcPath(edge),points=arcPoints(svg);
    assert.equal(points.length,97,'initial point and 96 native samples');
    assert.equal((svg.match(/M/g)||[]).length,1);
    assert.equal((svg.match(/L/g)||[]).length,96);
    assert.doesNotMatch(svg,/[ACZ]|NaN|Infinity/,'the concave flare is an open sampled path');
    for(const sample of nativeFlareSamples){
      const [x,y]=project[edge](sample.x,sample.y),actual=points[sample.i];
      close(actual.x*size,x*size,`${edge}/${size} sample ${sample.i} x`);
      close(actual.y*size,y*size,`${edge}/${size} sample ${sample.i} y`);
    }
    const radii=points.map(p=>Math.hypot(p.x,p.y));
    assert.ok(Math.max(...radii)-Math.min(...radii)>4,'a quarter circle cannot satisfy the flare samples');
  }
});

test('the resting arc keeps native clearance along each sampled normal',()=>{
  const points=arcPoints(N.settingsArcPath('right')),flare=103*44/117,clearance=39*44/117;
  for(const sample of nativeFlareSamples){
    const actual=points[sample.i],dx=actual.x-flare*(1-sample.v),dy=actual.y+flare*sample.u;
    close(Math.hypot(dx,dy),clearance,'constant normal clearance');
    close(dx*-Math.cos(sample.heading)+dy*Math.sin(sample.heading),clearance,'pocketward normal');
    close(dx*Math.sin(sample.heading)+dy*Math.cos(sample.heading),0,'no drift along native heading');
  }
});

test('actual handle placement draws the oriented flare once and preserves hit targets at every size',()=>{
  const placement=html.slice(html.indexOf('function put(el,point,z){'),html.indexOf('function near(at,x,y){'));
  assert.ok(placement.includes('function placeHandles(){'));
  const node=(children={})=>({innerHTML:'',style:{setProperty(name,value){this[name]=value;}},classList:{add(){},remove(){}},
    setAttribute(name,value){this[name]=value;},querySelector(selector){return children[selector];}});
  for(const edge of ['right','left','top','bottom'])for(const size of Object.values(contract.sizes)){
    const rest=node(),grip=node(),pill=rect(90,120,70,300);
    const context={Notch:N,NotchLayout:L,notchEdge:edge,pill,layoutRect:r=>r,
      orb:node({'.h-rest':rest}),moveHandle:node({'.h-glyph':grip}),showMove:true,orbAt:null,moveAt:null,
      document:{documentElement:{style:{zoom:String(size)}}}};
    assert.equal(vm.runInNewContext(placement+'\nplaceHandles();',context),true);
    const rendered=[...rest.innerHTML.matchAll(/<path d="([^"]+)"/g)];
    assert.equal(rendered.length,2,'existing stroke and fill layers both use the flare');
    for(const item of rendered)assert.equal(item[1],N.settingsArcPath(edge));
    assert.doesNotMatch(rest.innerHTML,/<circle|stroke-dasharray/);
    assert.equal((grip.innerHTML.match(/<circle /g)||[]).length,6);
    const handles=N.handleGeometry(edge,pill);
    for(const [name,target] of [['orb',context.orbAt],['move',context.moveAt]])for(const axis of ['x','y','reach'])close(target[axis],handles[name][axis]*size,`${name} target ${axis}`);
    const [x,y,w,h]=rest.viewBox.split(' ').map(Number),stroke=L.orbStroke+1;
    for(const point of arcPoints(rendered[0][1])){
      assert.ok(point.x-stroke/2>=x&&point.x+stroke/2<=x+w,'full round stroke fits horizontally');
      assert.ok(point.y-stroke/2>=y&&point.y+stroke/2<=y+h,'full round stroke fits vertically');
    }
  }
  for(const selector of ['#root #orb .h-rest','#root #orb.hover .h-rest']){
    const start=html.indexOf(selector+'{'),rule=html.slice(start,html.indexOf('}',start));
    assert.ok(start>=0);assert.doesNotMatch(rule,/rotate/,'path already has its native edge orientation');
    assert.match(rule,/transform:scale\(/,'hover retains only its existing scale transition');
  }
});

test('cards and tails keep native logical size at every notch size and all four edges',()=>{
  for(const edge of ['right','left','top','bottom'])for(const size of Object.values(contract.sizes)){
    const scale=1/size,p=N.vertical(edge)?rect(edge==='right'?390:0,150,L.sideBodyDepth,260):rect(160,edge==='top'?0:540,320,100);
    const ring=N.vertical(edge)?rect(p.left+L.sideRingMargin,220,44,44):rect(260,p.top+L.sideRingMargin,44,44);
    const g=N.cardGeometry({edge,width:650,height:650,pill:p,ring,card:{width:L.cardWidth*scale,height:240*scale},scale});
    close(g.card.width*size,L.cardWidth,'scale-neutral card');
    close((N.vertical(edge)?g.tail.width:g.tail.height)*size,L.tailLength,'scale-neutral tail');
    const gap=edge==='right'?p.left-(g.tail.left+g.tail.width):edge==='left'?g.tail.left-p.right:
      edge==='top'?g.tail.top-p.bottom:p.top-(g.tail.top+g.tail.height);
    close(gap*size,L.tailGap,'air gap');
    const anchor=N.vertical(edge)?g.tail.top+g.tail.height/2:g.tail.left+g.tail.width/2;
    close(anchor,N.vertical(edge)?ring.top+22:ring.left+22,'tip follows ring');
    assert.ok(g.bridge.width>0&&g.bridge.height>0,'air gap remains hot');
    assert.ok(g.card.left>=8&&g.card.top>=8);
    assert.ok(g.card.left+g.card.width<=642+1e-7&&g.card.top+g.card.height<=642+1e-7);
  }
});

test('tall content and edge providers stay inside taskbar insets instead of clipping headers or actions',()=>{
  for(const edge of ['right','left','top','bottom']){
    const p=N.vertical(edge)?rect(edge==='right'?390:0,100,L.sideBodyDepth,320):rect(100,edge==='top'?0:550,340,100);
    const room=N.cardSpace(edge,650,650,[24,16,48,20],p);
    for(const ring of [rect(p.left+10,p.top+4,44,44),rect(p.right-48,p.bottom-48,44,44)]){
      const g=N.cardGeometry({edge,width:650,height:650,insets:[24,16,48,20],pill:p,ring,card:{width:L.cardWidth,height:1200}});
      assert.ok(g.card.top>=room.topLimit&&g.card.left>=room.leftLimit);
      assert.ok(g.card.top+g.card.height<=room.bottomLimit+1e-7);
      assert.ok(g.card.left+g.card.width<=room.rightLimit+1e-7);
      assert.equal(g.card.height,room.maxHeight);
    }
  }
});

test('ordinary silhouettes use native curvature samples, reflected edges and the visible bezel band',()=>{
  for(const edge of ['right','left','top','bottom'])for(const size of Object.values(contract.sizes)){
    const g=N.silhouette(edge,L.sideBodyDepth,300,contract.rendering.bezelBleed/size);
    assert.equal((g.path.match(/C/g)||[]).length,2,'two rounded body corners');
    assert.ok((g.path.match(/L/g)||[]).length>=2*contract.shape.fluidTurnSteps,'two integrated flares');
    assert.doesNotMatch(g.path,/NaN|Infinity/);
    close(g.drawnFlare,Math.min(L.curlRadius,L.sideBodyDepth-L.cornerRadius-contract.rendering.bezelBleed/size));
    close(N.vertical(edge)?g.width:g.height,L.sideBodyDepth);
    close(N.vertical(edge)?g.height:g.width,300+2*L.curlRadius);
  }
});
