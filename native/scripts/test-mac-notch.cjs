const {test}=require('node:test');
const assert=require('node:assert/strict');
const {readFileSync}=require('node:fs');
const path=require('node:path');
const N=require('../matra/ui/matra-notch-reference.js');
const contract=require('../../shared/notch-reference/mac-notch-reference.json');
const L=N.reference.layout;
const close=(actual,expected,message)=>assert.ok(Math.abs(actual-expected)<1e-7,`${message||''}: ${actual} != ${expected}`);
const rect=(left,top,width,height)=>({left,top,right:left+width,bottom:top+height,width,height});

test('the shipped browser module embeds the generated native-source contract without edits',()=>{
  assert.deepEqual(N.reference,contract);
  const html=readFileSync(path.join(__dirname,'../matra/ui/notch.html'),'utf8');
  assert.match(html,/<script src="matra-notch-reference\.js"><\/script>/);
  for(const call of ['Notch.ringGeometry()','Notch.stackGeometry(','Notch.cardGeometry(','Notch.handleGeometry(','Notch.handleAt(','Notch.silhouette('])assert.ok(html.includes(call),call);
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
