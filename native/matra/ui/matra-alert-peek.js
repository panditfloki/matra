/* Notch notification presentation. Acknowledges only after a frame is rendered / audio is started.
   All strings become text nodes. It never opens a system notification or reads session contents. */
((root) => {
  const clean = value => String(value || '').replace(/[\u0000-\u001f\u007f]/g,' ').slice(0,180);
  async function tone(choice) {
    const Audio = root.AudioContext || root.webkitAudioContext;
    if (!Audio) throw new Error('Audio output is unavailable.');
    const context = new Audio();
    try {
      await Promise.race([context.resume(),new Promise((_,reject)=>setTimeout(()=>reject(new Error('Audio output did not start.')),500))]);
      if (context.state !== 'running') throw new Error('Audio output is blocked by this webview.');
      const frequencies = choice === 'chime' ? [660,880] : choice === 'soft' ? [440] : [600,720];
      frequencies.forEach((frequency,i)=>{
        const oscillator=context.createOscillator(), gain=context.createGain(); const at=context.currentTime+i*0.12;
        oscillator.type='sine'; oscillator.frequency.value=frequency;
        gain.gain.setValueAtTime(0,at); gain.gain.linearRampToValueAtTime(0.08,at+0.015); gain.gain.exponentialRampToValueAtTime(0.001,at+0.20);
        oscillator.connect(gain); gain.connect(context.destination); oscillator.start(at); oscillator.stop(at+0.22);
      });
      setTimeout(()=>context.close().catch(()=>{}),600);
    } catch (error) { context.close().catch(()=>{}); throw error; }
  }
  function mount({listen,invoke,anchor,edge,canShow,onChange,onGeometry=()=>{},play=tone,doc=root.document}) {
    const panel=doc.createElement('aside'); panel.className='matra-alert-peek'; panel.hidden=true;
    panel.setAttribute('role','status'); panel.setAttribute('aria-live','polite');
    Object.assign(panel.style,{position:'fixed',zIndex:'10000',boxSizing:'border-box',width:'320px',maxWidth:'calc(100vw - 16px)',padding:'15px',borderRadius:'16px',
      background:'var(--card-bg, #191919)',color:'var(--fg, #f6f6f6)',border:'1px solid rgba(128,128,128,.3)',boxShadow:'0 10px 30px #0005',font:'12px system-ui',pointerEvents:'auto'});
    const title=doc.createElement('strong'), body=doc.createElement('p'), close=doc.createElement('button');
    close.type='button'; close.textContent='Dismiss'; close.className='btn'; close.setAttribute('aria-label','Dismiss notification');
    Object.assign(close.style,{font:'inherit',color:'inherit',background:'#8882',border:'1px solid #8886',borderRadius:'7px',padding:'5px 9px',cursor:'pointer'});
    panel.append(title,body,close); doc.body.append(panel);
    let timer=null, disposed=false, revision=0, pending=null, unlisten=null;
    const ack=(id,error=null)=>Promise.resolve(invoke('acknowledge_notch_alert',{id,error})).catch(()=>{});
    function hide() {
      clearTimeout(timer); const wasVisible=!panel.hidden; panel.hidden=true;
      if(pending){revision++;ack(pending,'The notch alert closed before presentation finished.');pending=null;}
      if(wasVisible)onChange(false,0);
    }
    function position() {
      if (panel.hidden) return;
      const card=doc.getElementById('card');
      if(card){const style=root.getComputedStyle(card);panel.style.background=style.background;panel.style.color=style.color;}
      const r=anchor(), w=panel.getBoundingClientRect().width, h=panel.getBoundingClientRect().height;
      let x=r.left+(r.width-w)/2, y=r.bottom+12;
      if (edge()==='bottom') y=r.top-h-12;
      else if (edge()==='left') { x=r.right+12; y=r.top; }
      else if (edge()==='right') { x=r.left-w-12; y=r.top; }
      const zoom=parseFloat(doc.documentElement.style.zoom)||1;
      const left=`${Math.max(8,Math.min(root.innerWidth-w-8,x))/zoom}px`,top=`${Math.max(8,Math.min(root.innerHeight-h-8,y))/zoom}px`;
      if(panel.style.left!==left||panel.style.top!==top){panel.style.left=left;panel.style.top=top;onGeometry();}
    }
    async function receive(event) {
      const payload=event?.payload || {}, toast=payload.toast;
      if (disposed || typeof payload.id!=='string' || !toast || typeof toast!=='object') return;
      if (pending) { const old=pending; pending=null; ack(old,'A newer alert replaced this alert before it was presented.'); }
      const current=++revision; pending=payload.id;
      const finish=error=>{if(pending===payload.id){pending=null;return ack(payload.id,error);}};
      if (!canShow() || doc.visibilityState==='hidden') { finish('The notch is unavailable during a move or while hidden.'); return; }
      const duration=[3000,5000,10000].includes(toast.duration_ms)?toast.duration_ms:5000;
      if (toast.peek) {
        title.textContent=clean(toast.title); body.textContent=clean(toast.body); panel.hidden=false; position();
        onChange(true,Date.now()+duration); clearTimeout(timer); timer=setTimeout(hide,duration);
      }
      try {
        if (!toast.silent && toast.sound!=='off') await play(['default','soft','chime'].includes(toast.sound)?toast.sound:'default');
        if (toast.peek) await new Promise(resolve=>root.requestAnimationFrame(()=>root.requestAnimationFrame(resolve)));
        if (disposed || current!==revision) return;
        if (!toast.peek && (toast.silent || toast.sound==='off')) throw new Error('Notch peek and sound are both off.');
        if (!canShow() || doc.visibilityState==='hidden') throw new Error('The notch became unavailable before the alert was presented.');
        finish(null);
      } catch(error) { finish(clean(error?.message || 'The notch could not present this alert.')); }
    }
    close.addEventListener('click',hide);
    const visibility=()=>{if(doc.visibilityState==='hidden'){revision++;if(pending){ack(pending,'The notch was hidden.');pending=null;}hide();}};
    doc.addEventListener('visibilitychange',visibility); root.addEventListener('resize',position);
    const busyObserver=new MutationObserver(()=>{if(!canShow()){hide();}else position();});
    busyObserver.observe(doc.body,{attributes:true,attributeFilter:['class','data-edge']});
    busyObserver.observe(doc.documentElement,{attributes:true,attributeFilter:['class','style','data-matra-theme','data-theme']});
    Promise.resolve(listen('notch_alert',receive)).then(stop=>{if(disposed)stop();else unlisten=stop;}).catch(()=>{});
    return {receive,rect:()=>panel.hidden?null:panel.getBoundingClientRect(),hide,
      dispose(){disposed=true;revision++;if(pending){ack(pending,'The notch closed.');pending=null;}hide();unlisten?.();busyObserver.disconnect();root.removeEventListener('resize',position);doc.removeEventListener('visibilitychange',visibility);panel.remove();}};
  }
  root.MatraAlertPeek={mount,clean};
  if(typeof module!=='undefined')module.exports=root.MatraAlertPeek;
})(typeof window==='undefined'?globalThis:window);
