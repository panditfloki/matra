/* Native Mac measurements are generated from shared/notch-reference/mac-notch-reference.json.
   The functions below adapt those measurements to a WebView's measured font and viewport.
   They cover the ordinary flared notch, without the Mac hardware-cutout adapter. */
(function(root,factory){
  const value=factory();
  if(typeof module==='object'&&module.exports) module.exports=value;
  if(root) root.MatraNotchReference=value;
})(typeof window==='object'?window:null,function(){
  'use strict';
  const reference = /* BEGIN GENERATED MAC REFERENCE */ {"schemaVersion":1,"kind":"mac-notch-reference-data","units":{"layout":"logical-point","layoutExceptions":{"localArcMinimumSweep":"fraction-of-circle","orbMergeScale":"scale-factor","maxWindowCount":"count","sessionCeiling":"count","defaultSessionCap":"count"},"designPixels":"reference-frame-pixel","time":"second","axes":"top-left-origin-y-down"},"design":{"pointsPerDesignPixel":0.37606837606837606,"sfProCapRatio":0.714},"designPixels":{"sideBodyDepth":186,"curlRadius":103,"bezelFillet":28,"cornerRadius":78.8,"padTop":69.5,"padBottom":50.1,"cellSpacing":83.5,"pillWidth":26,"pillHeight":210,"pillHotZone":90,"ringDiameter":117,"trackStroke":15.5,"progressStroke":8,"glyphSize":46,"ringLabelGap":26.9,"activityDiameter":72,"activityStroke":5.5,"weeklyRingStroke":5,"weeklyInsideRadius":28,"weeklyOutsideRadius":65,"orbDiameter":124,"orbStroke":18,"orbGap":27,"orbClearance":39,"orbGlyph":56,"orbHotZone":152,"gripDot":19,"gripPitch":31,"gripGap":13,"gripHotZone":140,"cardWidth":600,"updateCardWidth":820,"cardCorner":49.5,"cardPadding":32,"tailLength":75,"tailHeight":87,"tailGap":28,"barHeight":10.5,"headerGap":17,"headerToBlock":21,"labelToBar":16.8,"barToUsed":17.8,"blockSpacing":20,"moneyBarHeight":12,"moneyBarToStats":14,"moneyStatGap":4,"usageDetailIdentityGap":4,"usageDetailBarHeight":10.5,"usageDetailLabelToBar":12,"usageDetailBarToStats":10,"usageDetailChartHeight":96,"usageDetailChartGap":18,"usageDetailBarGap":5,"sessionRowGap":10,"statusDot":17,"statusDotStroke":3.4,"statusDotGap":11,"hairline":2.5,"codexUsageTop":20,"codexMetricTop":14,"codexMetricRowGap":8,"codexMetricRowHeight":40,"codexMetricBottom":14,"codexUsageRowGap":12,"codexChartTop":15,"codexChartHeight":115,"splitRowGap":20,"splitRowSlack":24,"endSlack":190},"layout":{"sideBodyDepth":69.94871794871794,"curlRadius":38.73504273504273,"bezelFillet":10.52991452991453,"cornerRadius":29.634188034188032,"padTop":26.136752136752136,"padBottom":18.84102564102564,"cellSpacing":31.4017094017094,"pillWidth":9.777777777777779,"pillHeight":78.97435897435898,"pillHotZone":33.84615384615385,"ringDiameter":44,"splitCornerRadius":12,"trackStroke":5.829059829059829,"progressStroke":3.0085470085470085,"glyphSize":17.299145299145298,"ringLabelGap":10.116239316239316,"activityDiameter":27.076923076923077,"activityStroke":2.068376068376068,"weeklyRingStroke":1.8803418803418803,"weeklyInsideRadius":10.52991452991453,"weeklyOutsideRadius":24.444444444444443,"orbDiameter":46.63247863247863,"orbStroke":6.769230769230769,"orbGap":10.153846153846153,"orbClearance":14.666666666666666,"orbGlyph":21.05982905982906,"orbHotZone":57.16239316239316,"gripDot":7.145299145299145,"gripPitch":11.658119658119658,"gripWidth":18.8034188034188,"gripLength":30.46153846153846,"gripGap":4.888888888888889,"gripHotZone":52.64957264957265,"cardWidth":225.64102564102564,"updateCardWidth":308.37606837606836,"cardCorner":18.615384615384617,"cardPadding":12.034188034188034,"tailLength":28.205128205128204,"tailHeight":32.717948717948715,"tailGap":10.52991452991453,"barHeight":3.948717948717949,"headerGap":6.3931623931623935,"headerToBlock":7.897435897435898,"labelToBar":6.317948717948719,"barToUsed":6.694017094017094,"blockSpacing":7.521367521367521,"moneyBarHeight":4.512820512820513,"moneyBarToStats":5.264957264957265,"moneyStatGap":1.5042735042735043,"usageDetailIdentityGap":1.5042735042735043,"usageDetailBarHeight":3.948717948717949,"usageDetailLabelToBar":4.512820512820513,"usageDetailBarToStats":3.7606837606837606,"usageDetailChartHeight":36.1025641025641,"usageDetailChartGap":6.769230769230769,"usageDetailBarGap":1.8803418803418803,"sessionRowGap":3.7606837606837606,"statusDot":6.3931623931623935,"statusDotStroke":1.2786324786324785,"statusDotGap":4.136752136752136,"hairline":0.9401709401709402,"codexUsageTop":7.521367521367521,"codexMetricTop":5.264957264957265,"codexMetricRowGap":3.0085470085470085,"codexMetricRowHeight":15.042735042735043,"codexMetricHeight":87.24786324786325,"codexMetricBottom":5.264957264957265,"codexUsageRowGap":4.512820512820513,"codexChartTop":5.6410256410256405,"codexChartHeight":43.24786324786325,"splitRowGap":7.521367521367521,"splitRowSlack":9.025641025641026,"endSlack":71.45299145299145,"localArcMinimumSweep":0.06,"maxWindowCount":4,"sessionCeiling":12,"defaultSessionCap":4,"sideRingMargin":12.974358974358971,"orbArcRadius":28.58119658119658,"orbMergeScale":1.5921052631578947,"cardTextWidth":201.57264957264957,"orbInsetFromEdge":38.73504273504273},"typography":{"percentLabel":{"capDesignPixels":27,"fontSize":14.221073044602456},"cardTitle":{"capDesignPixels":26,"fontSize":13.694366635543108},"cardBody":{"capDesignPixels":18,"fontSize":9.480715363068304}},"motion":{"unfold":{"kind":"spring","responseSeconds":0.62,"dampingFraction":0.72},"lift":{"kind":"spring","responseSeconds":0.26,"dampingFraction":0.92},"contents":{"kind":"spring","responseSeconds":0.48,"dampingFraction":0.8},"glide":{"kind":"spring","responseSeconds":0.5,"dampingFraction":0.86},"crossfade":{"kind":"easeInOut","durationSeconds":0.16},"reading":{"kind":"spring","responseSeconds":0.9,"dampingFraction":0.9},"merge":{"kind":"easeIn","durationSeconds":0.2},"stagger":{"stepSeconds":0.045,"capSeconds":0.18},"reduceMotion":"none"},"shape":{"circleReach":0.5523,"filletRamp":0.5,"fluidTurnSteps":96},"rendering":{"bezelBleed":2,"contentRetreatDesignPixels":28,"contentRetreat":10.52991452991453,"unknownTrackDash":[3,3],"weeklyDash":[4,2],"activityArcFraction":0.25,"staleOpacity":0.45,"staleReducedTransparencyOpacity":0.75,"exhaustedGlyphOpacity":0.35,"exhaustedGlyphReducedTransparencyOpacity":0.7,"weeklyTrackOpacity":0.7,"weeklyTrackReducedTransparencyOpacity":1,"weeklyArcOpacity":0.8,"weeklyArcReducedTransparencyOpacity":1},"interaction":{"hoverGraceSeconds":0.25,"foldGraceSeconds":0.45,"settingsTurnBackSeconds":0.5,"gripGrowth":0.3},"sizes":{"small":0.8,"medium":1,"large":1.25},"anchors":{"right":{"isVertical":true,"tooltipDirection":"leading","outward":{"x":1,"y":0},"along":{"x":0,"y":1},"bezelAnchor":"trailing"},"left":{"isVertical":true,"tooltipDirection":"trailing","outward":{"x":-1,"y":0},"along":{"x":0,"y":1},"bezelAnchor":"leading"},"top":{"isVertical":false,"tooltipDirection":"down","outward":{"x":0,"y":-1},"along":{"x":1,"y":0},"bezelAnchor":"top"},"bottom":{"isVertical":false,"tooltipDirection":"up","outward":{"x":0,"y":1},"along":{"x":1,"y":0},"bezelAnchor":"bottom"}},"nativeMeasured":["percentLineHeight","cardTitleLineHeight","cardBodyLineHeight","wrappedTextHeight","screenAndCutoutGeometry"],"provenance":{"authority":"Existing native Mac source; this generated data does not replace its presentation or runtime.","generator":"scripts/check-mac-notch-reference.mjs","sources":[{"path":"native/macos/Sources/DesignSystem/Design.swift","sha256":"8d9a09843eb5ba6c3eb3765f1670042df78ca73f1f4bcc24a9bd3b682da0c652"},{"path":"native/macos/Sources/Notch/NotchLayout.swift","sha256":"bcae2960320c8fa9a91eb44f2588d3e3b99fc730da892e7359d7e71b314ac5b8"},{"path":"native/macos/Sources/Notch/NotchEdge.swift","sha256":"c1bf85eefe5149eb2bcb6ffd8111f11c98905690c67bb2f41b1c0794dbee2b0b"},{"path":"native/macos/Sources/Notch/NotchMotion.swift","sha256":"322fbaa939f615dac41b5864ddeeb566b804c277a92d24174d2ee558b423586a"},{"path":"native/macos/Sources/Notch/NotchWindowController.swift","sha256":"af06acc503fd012d410d6669cc575e4ca13c626a28f27e64c15d16fe41e28791"},{"path":"native/macos/Sources/Notch/NotchRootView.swift","sha256":"e913af3c467cf9072cf75c15dbe84120c3f066ac41f5d6377a37e447d1a834b0"},{"path":"native/macos/Sources/Settings/NotchSize.swift","sha256":"2c247236b56c3bf5c1e7a7ca0d9e28f9fde6fb35dec01baf0a96e713d3d8675c"},{"path":"native/macos/Sources/Notch/SideNotchShape.swift","sha256":"ee3812753f70754c75125e2220c07a12520528ccb02e81e70437cd83a119b312"},{"path":"native/macos/Sources/Features/SettingsHandle.swift","sha256":"579b1bcd542309ba824411374daacceafa7ce13d59c10314093cc936388e5a05"},{"path":"native/macos/Sources/Features/MoveHandle.swift","sha256":"aea46b1a3aba11e1dafee7d883d654e5c022bf6cab0e9e1c7ad6d0aa6190fbb4"},{"path":"native/macos/Sources/Features/ProviderRing.swift","sha256":"8c9036906618af40192de9a2aa5b42f9ecaed9b6caf93b80e8526e0fed3908d6"},{"path":"native/macos/Tests/NotchLayoutTests.swift","sha256":"7e6431ba8a1cc746fca82af08f68687029cc3b1bbe51eb0b1dc07560746d5d3b"},{"path":"native/macos/Tests/NotchEdgeTests.swift","sha256":"d611df051e60ecfcee8aec2c5582b49b49c2325052095285a37bb3d3ee71698c"}]}} /* END GENERATED MAC REFERENCE */;
  const L=reference.layout;
  const vertical=edge=>edge==='left'||edge==='right';
  const clamp=(value,min,max)=>Math.max(min,Math.min(Math.max(min,max),value));
  const finite=(value,fallback)=>Number.isFinite(value)?value:fallback;

  function ringGeometry(viewBox=56){
    const k=viewBox/L.ringDiameter;
    return {center:viewBox/2,diameter:viewBox,
      trackRadius:(L.ringDiameter-L.trackStroke)/2*k,trackStroke:L.trackStroke*k,
      progressStroke:L.progressStroke*k,weeklyStroke:L.weeklyRingStroke*k,
      weeklyInside:L.weeklyInsideRadius*k,weeklyOutside:L.weeklyOutsideRadius*k,
      activityRadius:L.activityDiameter/2*k,activityStroke:L.activityStroke*k};
  }

  // ProviderCell retains its line box when the reading is hidden. Font metrics
  // come from the actual platform; AppKit's computed line height is not guessed.
  function stackGeometry({edge='right',count=1,lineHeight,availableLength=Infinity}){
    count=Math.max(0,Math.trunc(finite(count,0)));
    lineHeight=Math.max(0,finite(lineHeight,0));
    const isVertical=vertical(edge),cellExtent=L.ringDiameter+L.ringLabelGap+lineHeight;
    const start=isVertical?L.padTop:(L.padTop+L.padBottom)/2;
    const end=isVertical?L.padBottom:start;
    const cellAlong=isVertical?cellExtent:L.ringDiameter;
    const packed=start+end+count*cellAlong;
    const spacing=count>1&&isVertical?clamp((availableLength-packed)/(count-1),0,L.cellSpacing):L.cellSpacing;
    return {depth:isVertical?L.sideBodyDepth:2*L.sideRingMargin+cellExtent,
      cellExtent,start,end,spacing,length:packed+Math.max(0,count-1)*spacing};
  }

  function handleGeometry(edge,pill){
    const reach=L.orbDiameter/2+L.gripGap+L.gripWidth/2;
    const isVertical=vertical(edge),along=isVertical?pill.bottom+L.curlRadius:pill.right+L.curlRadius;
    const across=edge==='right'?pill.right-L.curlRadius:edge==='left'?pill.left+L.curlRadius:
      edge==='top'?pill.top+L.curlRadius:pill.bottom-L.curlRadius;
    return {orb:{x:isVertical?across:along,y:isVertical?along:across,reach:L.orbHotZone/2},
      move:{x:isVertical?across:along+reach,y:isVertical?along+reach:across,reach:L.gripHotZone/2}};
  }
  function handleAt(point,handles,{folded=false,showMove=true}={}){
    if(folded) return null;
    const near=h=>h&&Math.hypot(point.x-h.x,point.y-h.y)<=h.reach;
    // Settings owns the intersection of its target and the grip's target.
    if(near(handles.orb)) return 'orb';
    return showMove&&near(handles.move)?'move':null;
  }
  function gripDots(edge){
    const points=[];
    for(const line of [-.5,.5])for(const row of [-1,0,1]){
      const along=line*L.gripPitch,across=row*L.gripPitch;
      points.push({x:vertical(edge)?across:along,y:vertical(edge)?along:across,r:L.gripDot/2});
    }
    return points;
  }

  function cardSpace(edge,width,height,insets,pill,scale=1){
    const [top,right,bottom,left]=insets;
    const margin=8,clearance=(L.tailLength+L.tailGap)*scale;
    const leftLimit=edge==='left'?Math.max(left+margin,pill.right+clearance):left+margin;
    const rightLimit=edge==='right'?Math.min(width-right-margin,pill.left-clearance):width-right-margin;
    const topLimit=edge==='top'?Math.max(top+margin,pill.bottom+clearance):top+margin;
    const bottomLimit=edge==='bottom'?Math.min(height-bottom-margin,pill.top-clearance):height-bottom-margin;
    return {leftLimit,rightLimit,topLimit,bottomLimit,maxWidth:Math.max(0,rightLimit-leftLimit),maxHeight:Math.max(0,bottomLimit-topLimit)};
  }
  function cardGeometry({edge,width,height,insets=[0,0,0,0],pill,ring,card,scale=1}){
    const room=cardSpace(edge,width,height,insets,pill,scale),v=vertical(edge);
    const w=Math.min(card.width,room.maxWidth),h=Math.min(card.height,room.maxHeight);
    const cx=(ring.left+ring.right)/2,cy=(ring.top+ring.bottom)/2;
    const left=v?(edge==='left'?room.leftLimit:room.rightLimit-w):clamp(cx-w/2,room.leftLimit,room.rightLimit-w);
    const top=v?clamp(cy-h/2,room.topLimit,room.bottomLimit-h):(edge==='top'?room.topLimit:room.bottomLimit-h);
    const length=L.tailLength*scale,span=L.tailHeight*scale,corner=L.cardCorner*scale;
    const center=v?clamp(cy,top+corner+span/2,top+h-corner-span/2):clamp(cx,left+corner+span/2,left+w-corner-span/2);
    const tail={left:v?(edge==='left'?left-length:left+w):center-span/2,
      top:v?center-span/2:(edge==='top'?top-length:top+h),width:v?length:span,height:v?span:length};
    // Windows click-through must include the intentional air gap at the tip.
    const bridge=v?{left:Math.min(pill.left,tail.left),top:tail.top,width:Math.max(pill.right,tail.left+tail.width)-Math.min(pill.left,tail.left),height:tail.height}:
      {left:tail.left,top:Math.min(pill.top,tail.top),width:tail.width,height:Math.max(pill.bottom,tail.top+tail.height)-Math.min(pill.top,tail.top)};
    return {card:{left,top,width:w,height:h},tail,bridge,room};
  }
  function tailPath(edge,width,height){
    const x=width,y=height;
    if(edge==='right')return `M0 0C0 ${y*.25} ${x*.58} ${y*.38} ${x} ${y/2}C${x*.58} ${y*.62} 0 ${y*.75} 0 ${y}Z`;
    if(edge==='left')return `M${x} 0C${x} ${y*.25} ${x*.42} ${y*.38} 0 ${y/2}C${x*.42} ${y*.62} ${x} ${y*.75} ${x} ${y}Z`;
    if(edge==='top')return `M0 ${y}C${x*.25} ${y} ${x*.38} ${y*.42} ${x/2} 0C${x*.62} ${y*.42} ${x*.75} ${y} ${x} ${y}Z`;
    return `M0 0C${x*.25} 0 ${x*.38} ${y*.58} ${x/2} ${y}C${x*.62} ${y*.58} ${x*.75} 0 ${x} 0Z`;
  }

  // SideNotchShape.canonicalPath, ordinary (no hardware cutout) case. The flare
  // integrates the same curvature ramp, rather than substituting a circular arc.
  function silhouette(edge,depth,length,bleed=0){
    const S=reference.shape,corner=Math.max(0,Math.min(L.cornerRadius,depth/2));
    const band=Math.max(0,Math.min(bleed,depth-corner));
    const curl=Math.max(0,Math.min(L.curlRadius,depth-corner-band));
    const extent=length+2*L.curlRadius,commands=[];
    const point=(x,y)=>edge==='left'?[depth-x,y]:edge==='top'?[y,depth-x]:edge==='bottom'?[y,x]:[x,y];
    const coords=p=>p.map(n=>Number(n.toFixed(5))).join(' ');
    const move=(x,y)=>commands.push('M'+coords(point(x,y)));
    const line=(x,y)=>commands.push('L'+coords(point(x,y)));
    const curve=(a,b,end)=>commands.push('C'+coords(point(...a))+' '+coords(point(...b))+' '+coords(point(...end)));
    function fluid(from,to,leaving,arriving){
      const p=S.filletRamp,bend=(Math.PI/2)/(1-p),walk=[];let heading=0,u=0,v=0;
      for(let i=0;i<S.fluidTurnSteps;i++){
        const s=(i+.5)/S.fluidTurnSteps,share=p<=0?1:s<p?s/p:s>1-p?(1-s)/p:1;
        heading+=bend*share/S.fluidTurnSteps;u+=Math.cos(heading)/S.fluidTurnSteps;v+=Math.sin(heading)/S.fluidTurnSteps;walk.push([u,v]);
      }
      const a=(to[0]-from[0])*leaving[0]+(to[1]-from[1])*leaving[1];
      const b=(to[0]-from[0])*arriving[0]+(to[1]-from[1])*arriving[1];
      for(const [wu,wv] of walk)line(from[0]+leaving[0]*wu*a/u+arriving[0]*wv*b/v,from[1]+leaving[1]*wu*a/u+arriving[1]*wv*b/v);
    }
    const c=Math.min(corner,length/2),k=S.circleReach;
    move(depth,0);if(band)line(depth-band,0);
    fluid([depth-band,0],[depth-band-curl,curl],[0,1],[-1,0]);line(c,curl);
    curve([c-c*k,curl],[0,curl+c-c*k],[0,curl+c]);line(0,extent-curl-c);
    curve([0,extent-curl-c+c*k],[c-c*k,extent-curl],[c,extent-curl]);line(depth-band-curl,extent-curl);
    fluid([depth-band-curl,extent-curl],[depth-band,extent],[1,0],[0,1]);if(band)line(depth,extent);commands.push('Z');
    return {path:commands.join(''),width:vertical(edge)?depth:extent,height:vertical(edge)?extent:depth,flare:L.curlRadius,drawnFlare:curl};
  }
  return {reference,vertical,ringGeometry,stackGeometry,handleGeometry,handleAt,gripDots,cardSpace,cardGeometry,tailPath,silhouette};
});
