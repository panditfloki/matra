/* CSS-to-physical coordinate scale for the notch window.
   Native hot rectangles, the carry pointer/body centre and the landing centre are reported in
   window-relative physical pixels. WebView2 folds its page zoom into devicePixelRatio, so DPR is
   that scale there. WebKitGTK does not: its page zoom leaves DPR at the GDK scale while the CSS
   viewport grows, so DPR overstates physical size by 1/zoom (CI: DPR 1, zoom .8, viewport 460 for
   a 368 px window). The grounded value on both engines is the native window's physical inner size
   divided by the CSS viewport measured against it. Root CSS `zoom` does not change innerWidth or
   the viewport coordinates getBoundingClientRect and clientX already use, so it is not part of the
   factor (callers that hold unzoomed layout lengths still apply it themselves). Raw DPR is kept for
   report_dpr, which corrects the WebView zoom and must see the engine's own value. */
(function(root,factory){const api=factory();if(typeof module==='object'&&module.exports)module.exports=api;if(root)root.MatraNativeScale=api;})(typeof window==='object'?window:null,function(){
  'use strict';
  const MAX_ATTEMPTS=3;
  const finite=n=>typeof n==='number'&&Number.isFinite(n);
  /* One physical size against one CSS viewport. innerWidth/innerHeight are integers, so each axis
     carries up to one CSS px of rounding; the two axes must agree within that, or the window and
     the page were caught mid-resize (different shapes) and the pair is not a measurement. */
  function ratio(physical,viewport){
    if(!physical||!viewport)return null;
    const pw=Number(physical.width),ph=Number(physical.height),cw=Number(viewport[0]),ch=Number(viewport[1]);
    if(![pw,ph,cw,ch].every(finite)||pw<=0||ph<=0||cw<=0||ch<=0)return null;
    const kw=pw/cw,kh=ph/ch,k=(pw+ph)/(cw+ch);
    const slack=k*(1/cw+1/ch)*1.5+0.002;
    if(Math.abs(kw-kh)>slack)return null;
    if(k<0.1||k>8)return null;
    return{k,slack};
  }
  /* Where the engine's DPR already is the physical scale (WebView2 at any zoom, WebKitGTK at zoom 1)
     the measurement agrees with it to within rounding; DPR is then used exactly, so those hosts keep
     their previous coordinates bit for bit. Anything further off is the engine's page zoom. */
  function resolve(physical,viewport,dpr){
    const r=ratio(physical,viewport);
    if(!r)return null;
    const d=finite(dpr)&&dpr>0?dpr:1;
    return Math.abs(r.k-d)<=r.slack+0.003?{k:d,dprExact:true}:{k:r.k,dprExact:false};
  }
  function rectToPhysical(r,k){return[r.left*k,r.top*k,r.width*k,r.height*k];}

  /* measure(): Promise<{width,height}> of the window's physical inner size. A host with no measure
     function at all (the browser fixture) cannot say: DPR is then the only grounded value and the
     scale is ready at once. Where measure exists, an error, a null answer or a malformed/inconsistent
     size is a failed measurement, never readiness; each run retries it a bounded number of times.
     onChange(k) runs once per accepted change of the factor, never for an unchanged one.
     onAccept(k,changed) runs once per accepted measurement, changed or not, so a caller that withheld
     geometry until ready can report it exactly once. onFail(reason) runs when a run ends without one. */
  const RETRY_MS=25;
  function create({dpr,viewport,measure,onChange,onAccept,onFail,log,wait}={}){
    const readDpr=()=>{const d=dpr?dpr():1;return finite(d)&&d>0?d:1;};
    const readViewport=()=>viewport?viewport():[0,0];
    const pause=wait||(ms=>new Promise(resolve=>setTimeout(resolve,ms)));
    const say=m=>{if(log)try{log(m);}catch(_){}};
    // A caller's hook must not turn an accepted measurement into a rejected promise.
    const call=(fn,...a)=>{if(fn)try{fn(...a);}catch(error){say('native scale: callback failed: '+(error&&error.message||error));}};
    let epoch=0,accepted=null,attempted=null,inflight=null,unavailable=typeof measure!=='function';
    const keyNow=()=>{const v=readViewport();return v[0]+'x'+v[1]+'@'+readDpr()+'#'+epoch;};
    const current=()=>!!accepted&&accepted.key===keyNow();
    function factor(){
      if(current())return accepted.k;
      const d=readDpr();
      // Before a measurement, or where none is possible: the engine's own DPR (unchanged behaviour).
      if(!accepted||accepted.dprExact)return d;
      // Stale on a zooming engine: keep the measured page zoom and follow any DPR change, which is
      // much nearer than raw DPR; the fresh measurement already under way replaces it.
      return accepted.k*d/accepted.dpr;
    }
    function ready(){return unavailable||current();}
    function invalidate(){epoch++;attempted=null;}
    function accept(result,key,d){
      const previous=factor(),changed=Math.abs(previous-result.k)>1e-6;
      accepted={k:result.k,dprExact:result.dprExact,dpr:d,key};attempted=null;
      if(changed)call(onChange,result.k);
      call(onAccept,result.k,changed);
      return result.k;
    }
    const describe=p=>p&&typeof p==='object'?`${p.width}x${p.height}`:String(p);
    async function run(){
      let reason='no measurement';
      for(let attempt=0;attempt<MAX_ATTEMPTS;attempt++){
        const key=keyNow(),view=readViewport(),d=readDpr();
        let physical,failed=false;
        try{physical=await measure();}
        catch(error){reason='measurement failed: '+(error&&error.message||error);failed=true;}
        // The page or the window changed while the native answer was in flight: that answer belongs
        // to a page that no longer exists, whatever it says. Measure the page as it is now.
        if(keyNow()!==key){reason='the page changed during measurement';continue;}
        if(!failed){
          const result=resolve(physical,view,d);
          if(result)return accept(result,key,d);
          reason=`rejected ${describe(physical)} against ${view[0]}x${view[1]} dpr=${d}`;
        }
        say('native scale: '+reason);
        // A real host that errs or answers nonsense is retried, briefly and a bounded number of times.
        if(attempt+1<MAX_ATTEMPTS)await pause(RETRY_MS*(attempt+1));
      }
      attempted=keyNow();
      call(onFail,reason);
      return factor();
    }
    function refresh(){
      if(unavailable)return Promise.resolve(factor());
      if(!inflight)inflight=run().finally(()=>{inflight=null;});
      return inflight;
    }
    // Measure only when needed and only once per state: a rejected or failed state waits for the next
    // resize/scale event instead of being retried by every hot-rectangle report.
    function ensure(){
      if(unavailable||current()||inflight||attempted===keyNow())return false;
      refresh().catch(()=>{});return true;
    }
    return{
      factor,ready,refresh,ensure,invalidate,
      // The page state a measurement belongs to (viewport, DPR, native epoch): a caller that holds CSS
      // coordinates while waiting compares it to know whether they still mean the same physical spot.
      key:keyNow,
      rect:el=>rectToPhysical(el.getBoundingClientRect(),factor()),
      box:r=>rectToPhysical(r,factor()),
      point:(x,y)=>{const k=factor();return[x*k,y*k];},
      state:()=>({factor:factor(),ready:ready(),unavailable,measured:accepted&&{...accepted},epoch,inflight:!!inflight})
    };
  }
  return{ratio,resolve,create,rectToPhysical};
});
