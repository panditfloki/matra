/* Perimeter presentation and gesture latch. Native Rust owns pointer/capture,
   springs and persistence. This file contains no provider fixtures or fake rings. */
(function(root,factory){const api=factory();if(typeof module==='object'&&module.exports)module.exports=api;if(root)root.MatraCarry=api;})(typeof window==='object'?window:null,function(){
  'use strict';
  const edges=['top','right','bottom','left'],clamp=(n,a,b)=>Math.max(a,Math.min(b,n));
  const vertical=e=>e==='right'||e==='left';
  const corners={topRight:['top','right'],bottomRight:['right','bottom'],bottomLeft:['bottom','left'],topLeft:['left','top']};
  function wrap(t,w,h){const n=2*(w+h);return ((t%n)+n)%n;}
  function place(t,w,h){t=wrap(t,w,h);return t<w?{edge:'top',along:t}:t<w+h?{edge:'right',along:t-w}:t<2*w+h?{edge:'bottom',along:2*w+h-t}:{edge:'left',along:2*w+2*h-t};}
  function cornerAt(c,w,h){return {topRight:w,bottomRight:w+h,bottomLeft:2*w+h,topLeft:0}[c];}

  // Native SideNotchShape canonical path without the Mac hardware-cutout branch.
  // Full length includes flares. A joined corner end closes its flare and corner
  // progressively; no rounded rectangle is substituted during corner passage.
  function bodyPath(edge,depth,length,flare,cornerRadius,bleed,leadJoin=0,trailJoin=0){
    const wanted=Math.max(0,Math.min(cornerRadius,depth/2)),band=clamp(bleed,0,depth-wanted);
    const curl=clamp(flare,0,depth-wanted-band),open=1-clamp(leadJoin,0,1),trail=1-clamp(trailJoin,0,1);
    const leadCurl=curl*open,trailCurl=curl*trail,corner=Math.max(0,Math.min(wanted,(length-leadCurl-trailCurl)/Math.max(1,open+trail)));
    const a=corner*open,b=corner*trail,commands=[];
    const point=(x,y)=>edge==='left'?[depth-x,y]:edge==='top'?[y,depth-x]:edge==='bottom'?[y,x]:[x,y];
    const pair=p=>p.map(n=>Number(n.toFixed(5))).join(' ');
    const move=(x,y)=>commands.push('M'+pair(point(x,y))),line=(x,y)=>commands.push('L'+pair(point(x,y)));
    const curve=(a,b,c)=>commands.push('C'+pair(point(...a))+' '+pair(point(...b))+' '+pair(point(...c)));
    function fluid(from,to,leaving,arriving){
      let heading=0,u=0,v=0;const walk=[],steps=96,ramp=.5,bend=(Math.PI/2)/(1-ramp);
      for(let i=0;i<steps;i++){const s=(i+.5)/steps,share=s<ramp?s/ramp:s>1-ramp?(1-s)/ramp:1;heading+=bend*share/steps;u+=Math.cos(heading)/steps;v+=Math.sin(heading)/steps;walk.push([u,v]);}
      const a=(to[0]-from[0])*leaving[0]+(to[1]-from[1])*leaving[1],b=(to[0]-from[0])*arriving[0]+(to[1]-from[1])*arriving[1];
      for(const[x,y]of walk)line(from[0]+leaving[0]*x*a/u+arriving[0]*y*b/v,from[1]+leaving[1]*x*a/u+arriving[1]*y*b/v);
    }
    const k=.5523;move(depth,0);if(band)line(depth-band,0);
    if(leadCurl>.001)fluid([depth-band,0],[depth-band-leadCurl,leadCurl],[0,1],[-1,0]);
    line(a,leadCurl);curve([a-a*k,leadCurl],[0,leadCurl+a-a*k],[0,leadCurl+a]);
    line(0,length-trailCurl-b);curve([0,length-trailCurl-b+b*k],[b-b*k,length-trailCurl],[b,length-trailCurl]);
    line(depth-band-trailCurl,length-trailCurl);
    if(trailCurl>.001)fluid([depth-band-trailCurl,length-trailCurl],[depth-band,length],[1,0],[0,1]);
    if(band)line(depth,length);commands.push('Z');return commands.join('');
  }
  function parts(frame,p){
    const k=p.scale,flare=p.flare*k,corner=p.corner_radius*k,bleed=p.bleed,margin=40;
    const depth=edge=>bleed+(p.layouts[edges.indexOf(edge)].depth*k-bleed)*frame.thin;
    const rect=(edge,low,length,d)=>{
      if(edge==='top')return{x:low,y:-bleed-margin};
      if(edge==='bottom')return{x:low,y:frame.h+bleed+margin-d};
      if(edge==='left')return{x:-bleed-margin,y:low};
      return{x:frame.w+bleed+margin-d,y:low};
    };
    if(!frame.corner){const a=place(frame.place-frame.length/2+.001,frame.w,frame.h),b=place(frame.place+frame.length/2-.001,frame.w,frame.h),d=depth(frame.from)+margin;
      return[{...rect(frame.from,Math.min(a.along,b.along),frame.length,d),path:bodyPath(frame.from,d,frame.length,flare,corner,bleed+margin)}];}
    const at=cornerAt(frame.corner,frame.w,frame.h),[before,after]=corners[frame.corner],visible=Math.max(1,depth(before)-bleed);
    const piece=(edge,len,isBefore,other)=>{
      const start=isBefore?at-len:at,end=isBefore?at:at+len;
      const a=place(start+.001,frame.w,frame.h),b=place(end-.001,frame.w,frame.h);
      let low=Math.min(a.along,b.along),high=Math.max(a.along,b.along);
      const lowJoin=(edge==='top'&&frame.corner==='topLeft')||(edge==='left'&&frame.corner==='topLeft')||(edge==='right'&&frame.corner==='topRight')||(edge==='bottom'&&frame.corner==='bottomLeft');
      const run=clamp(other,0,margin);if(lowJoin)low-=run;else high+=run;
      const d=depth(edge)+margin,join=clamp(other/visible,0,1),length=Math.max(.001,high-low);
      return{...rect(edge,low,length,d),path:bodyPath(edge,d,length,flare,corner,bleed+margin,lowJoin?join:0,lowJoin?0:join)};
    };
    return[frame.before>.5?piece(before,frame.before,true,frame.after):null,frame.after>.5?piece(after,frame.after,false,frame.before):null].filter(Boolean);
  }
  function goo(frame,p){if(!frame.corner)return 0;const share=Math.min(1,4*Math.min(frame.before,frame.after)/frame.length);return(p.layouts[edges.indexOf(frame.from)].depth*p.scale-p.bleed)*.28*share*share*(3-2*share);}

  // Flatten the actual SVG ring layers, glyph and reading into one inert SVG.
  // Nested foreignObject images decode but are blank in some WebViews, so carry
  // does not depend on HTML rendering inside an image document.
  function snapshotCell(cell){
    const ns='http://www.w3.org/2000/svg',z=parseFloat(document.documentElement.style.zoom)||1,base=cell.getBoundingClientRect(),pad=6;
    const width=base.width/z,height=base.height/z,root=document.createElementNS(ns,'svg');
    root.setAttribute('xmlns',ns);root.setAttribute('width',width+pad*2);root.setAttribute('height',height+pad*2);root.setAttribute('data-provider',cell.dataset.p||'');
    const box=el=>{const r=el.getBoundingClientRect();return{x:(r.left-base.left)/z+pad,y:(r.top-base.top)/z+pad,width:r.width/z,height:r.height/z};};
    const opacity=el=>{let value=1;for(let at=el;at&&at!==cell;at=at.parentElement)value*=Number(getComputedStyle(at).opacity);return value;};
    function svg(el){
      const clone=el.cloneNode(true),originals=[el,...el.querySelectorAll('*')],copies=[clone,...clone.querySelectorAll('*')];
      originals.forEach((node,i)=>{const css=getComputedStyle(node),to=copies[i];
        for(const a of [...to.attributes])if(/^on/i.test(a.name))to.removeAttribute(a.name);
        for(const key of ['fill','fill-rule','stroke','stroke-width','stroke-linecap','stroke-linejoin','stroke-dasharray','stroke-dashoffset','opacity','color','display','transform','transform-origin'])to.style.setProperty(key,css.getPropertyValue(key));
        to.style.setProperty('animation','none','important');to.style.setProperty('transition','none','important');
      });
      const r=box(el);for(const[k,v]of Object.entries(r))clone.setAttribute(k,v);
      clone.style.cssText='overflow:visible;opacity:'+opacity(el);root.appendChild(clone);
    }
    for(const el of cell.querySelectorAll('.ringwrap > svg'))svg(el);
    const glyph=cell.querySelector('.glyph');
    if(glyph){
      const mark=glyph.querySelector('svg'),img=glyph.querySelector('img');
      if(mark)svg(mark);
      else if(img){const src=img.getAttribute('src')||'';if(!/^data:image\//.test(src))throw Error('Carry glyph is not an embedded image');
        const image=document.createElementNS(ns,'image');for(const[k,v]of Object.entries(box(img)))image.setAttribute(k,v);image.setAttribute('href',src);image.setAttribute('opacity',opacity(img));root.appendChild(image);
      }else text(glyph);
    }
    function text(el){const css=getComputedStyle(el);if(css.display==='none'||css.visibility==='hidden')return;
      const r=box(el),label=document.createElementNS(ns,'text');
      for(const[k,v]of Object.entries({x:r.x+r.width/2,y:r.y+r.height/2,'text-anchor':'middle','dominant-baseline':'central','font-family':css.fontFamily,'font-size':css.fontSize,'font-weight':css.fontWeight,'letter-spacing':css.letterSpacing,fill:css.color,opacity:opacity(el)}))label.setAttribute(k,v);
      label.textContent=el.textContent;root.appendChild(label);
    }
    const reading=cell.querySelector('.pct');if(reading)text(reading);
    return{image:'data:image/svg+xml;charset=utf-8,'+encodeURIComponent(new XMLSerializer().serializeToString(root)),width:width+pad*2,height:height+pad*2};
  }

  // Exactly-once frontend cleanup, including terminal IPC arriving before begin's
  // promise, stale terminal messages, rejected begin, and rapid release.
  class Lifecycle {
    constructor(onEnd){this.id=0;this.onEnd=onEnd;this.phase='idle';this.sequence=0;this.lastId=0;}
    begin(){if(this.id)return null;this.id=Math.max(Date.now()*1000+(++this.sequence%1000),this.lastId+1);this.lastId=this.id;this.phase='starting';return this.id;}
    present(id){if(this.id!==id)return false;this.phase='dragging';return true;}
    release(){if(!this.id)return null;this.phase='settling';return this.id;}
    finish(event){if(!event||event.id!==this.id||!this.id)return false;this.id=0;this.phase='idle';this.onEnd(event);return true;}
    active(){return this.id!==0;}
  }
  return{edges,vertical,wrap,place,cornerAt,bodyPath,parts,goo,snapshotCell,Lifecycle};
});
