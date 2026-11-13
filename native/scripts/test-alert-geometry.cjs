/* Shipped alert renderer regression using native CI geometry and actual browser CSSOM mutations.
   No native app, notification service, account or network access. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const puppeteer = require(process.env.MATRA_PUPPETEER_PATH || 'puppeteer');
const repo = path.resolve(__dirname, '../..');
const source = path.join(repo, 'native/matra/ui/matra-alert-peek.js');
const label = process.argv[2] || 'after';
assert.match(label, /^[a-z0-9-]+$/);
const out = path.join(repo, '.runtime-build/alert-browser', label);
fs.mkdirSync(out, {recursive: true});
assert.ok(fs.realpathSync(out).startsWith(fs.realpathSync(repo) + path.sep));
const report = {moduleSha256: crypto.createHash('sha256').update(fs.readFileSync(source)).digest('hex'), harnessSha256: crypto.createHash('sha256').update(fs.readFileSync(__filename)).digest('hex'), checks: [], cases: [], failures: [], errors: [], network: []};
(async () => {
  const browser = await puppeteer.launch({
    executablePath: process.env.MATRA_CHROME_PATH, headless: true, userDataDir: path.join(out, 'profile'), protocolTimeout: 5000,
    env: {...process.env, TMPDIR: path.join(repo, '.runtime-build/tmp'), XDG_CACHE_HOME: path.join(out, 'cache')},
    args: ['--disable-background-networking', '--disable-component-update', '--no-first-run', '--no-default-browser-check', '--disable-crash-reporter', '--disable-breakpad', '--disable-sync', '--disable-features=MediaRouter']
  });
  try {
    const page = await browser.newPage();
    page.setDefaultTimeout(5000);
    page.on('pageerror', error => report.errors.push(error.message));
    await page.setRequestInterception(true);
    page.on('request', request => {
      if (/^https?:/.test(request.url())) { report.network.push(request.url()); request.abort(); }
      else request.continue();
    });
    await page.setViewport({width: 460, height: 650, deviceScaleFactor: 1});
    await page.setContent('<style>body{margin:0}#anchor{position:fixed;left:390.0625px;top:265.96875px;width:69.9375px;height:118.0625px}#card{background:rgb(25,25,25);color:rgb(246,246,246)}</style><div id="anchor"></div><div id="card"></div>');
    await page.addScriptTag({path: source});
    // The callback performs the same five documentElement CSS custom-property writes used by
    // notch.html updateNotchGeometry/reportHot. Browser CSSOM and MutationObserver stay real.
    await page.evaluate(() => {
      window.runAlertCase = async (edge, zoom, feedback) => {
        document.documentElement.style.zoom = String(zoom);
        const anchor = document.querySelector('#anchor');
        anchor.style.top = '265.96875px';
        let geometry = 0, guardTripped = false, timerFired = false, frames = 0, busy = false;
        const acks = [], changes = [];
        const refreshNativeHotRegion = () => {
          geometry++;
          // Stop a broken module from permanently starving this test process. The assertion
          // below rejects any guard activation; production code is not instrumented or changed.
          if (geometry > 256) { guardTripped = true; return; }
          for (const [name, value] of Object.entries({
            '--notch-percent-line-height': '15px', '--notch-cell-extent': '34px',
            '--notch-cell-spacing': '5px', '--notch-body-depth': '69.9375px',
            '--notch-available-length': '489.0625px'
          })) document.documentElement.style.setProperty(name, value);
          // Also cover hosts which emit a style mutation when reapplying an unchanged layout.
          // setAttribute preserves the exact visual style but guarantees a real mutation record.
          if (feedback === 'attribute') document.documentElement.setAttribute('style', document.documentElement.getAttribute('style'));
        };
        const view = MatraAlertPeek.mount({listen: async () => () => {}, invoke: async (_, args) => acks.push(args),
          anchor: () => anchor.getBoundingClientRect(), edge: () => edge, canShow: () => !busy,
          onGeometry: refreshNativeHotRegion, onChange: (visible, until) => changes.push({visible, until})});
        const turn = () => new Promise(resolve => requestAnimationFrame(() => { frames++; resolve(); }));
        setTimeout(() => { timerFired = true; }, 0);
        await view.receive({payload: {id: `${edge}/${zoom}`, toast: {title: 'Native geometry fixture', body: 'Fractional layout must leave the renderer responsive.', peek: true, silent: true, duration_ms: 10000}}});
        await turn(); await turn();
        const panel = document.querySelector('.matra-alert-peek');
        const rect = () => { const r = panel.getBoundingClientRect(); return {left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height}; };
        const shown = rect(), initialCalls = geometry;
        const css = {left: panel.style.left, top: panel.style.top};
        // Actual root attribute mutations arrive in bursts during native resize/theme reporting.
        for (let i = 0; i < 40; i++) document.documentElement.style.setProperty('--native-style-fixture', String(i));
        await turn(); await turn(); await turn();
        const stableCalls = geometry, stable = rect();
        anchor.style.top = '201.515625px';
        document.body.classList.toggle('native-geometry-fixture');
        await turn(); await turn(); await turn();
        const moved = rect(), movedCalls = geometry;
        // A queued geometry refresh must not resurrect a hidden alert or publish stale hit areas.
        document.documentElement.style.setProperty('--native-style-fixture', 'pending-hide');
        await Promise.resolve(); // Deliver MutationObserver and queue its frame before hiding.
        view.hide();
        const atHide = geometry;
        await turn(); await turn();
        const hidden = panel.hidden && view.rect() === null;
        const afterHide = geometry;
        // Carry class changes must still hide immediately and leave one terminal acknowledgement.
        await view.receive({payload: {id: `busy/${edge}/${zoom}`, toast: {title: 'Carry cancellation fixture', peek: true, silent: true}}});
        busy = true; document.body.classList.toggle('carrying');
        await turn(); await turn();
        const hiddenForCarry = panel.hidden && view.rect() === null;
        const atDispose = geometry;
        document.documentElement.style.setProperty('--native-style-fixture', 'pending-dispose');
        await Promise.resolve();
        view.dispose();
        await turn(); await turn();
        return {edge, zoom, feedback, viewport: innerWidth, shown, css, initialCalls, stableCalls, stable, moved, movedCalls,
          guardTripped, timerFired, frames, acks, changes, hidden, atHide, afterHide, hiddenForCarry,
          disposed: !panel.isConnected, atDispose, afterDispose: geometry};
      };
    });
    const layouts = [{width:460,zoom:1},{width:459,zoom:459/460},{width:407,zoom:407/460},{width:460,zoom:1.25},{width:460,zoom:0.8}];
    for (const edge of ['right', 'left', 'top', 'bottom']) for (const {width,zoom} of layouts) for (const feedback of ['properties','attribute']) {
      await page.setViewport({width, height:650, deviceScaleFactor:1});
      const result = await page.evaluate((edge, zoom, feedback) => runAlertCase(edge, zoom, feedback), edge, zoom, feedback);
      report.cases.push(result);
      try {
      assert.equal(result.guardTripped, false, `${edge}/${zoom}: geometry feedback loop exceeded 256 callbacks`);
      assert.ok(result.initialCalls <= 3, `${edge}/${zoom}: unstable first presentation (${result.initialCalls} callbacks)`);
      assert.equal(result.stableCalls, result.initialCalls, `${edge}/${zoom}: unchanged root styles republished geometry`);
      assert.deepEqual(result.stable, result.shown);
      assert.ok(result.timerFired && result.frames >= 10, 'rendering and timer tasks remain runnable');
      assert.equal(result.acks.length, 2);
      assert.ok(result.acks.every(ack => ack.error === null));
      assert.equal(new Set(result.acks.map(ack => ack.id)).size, 2);
      assert.ok(result.shown.left >= 0 && result.shown.top >= 0 && result.shown.right <= width+1 && result.shown.bottom <= 651);
      assert.notEqual(result.moved.top, result.shown.top, 'real anchor movement is applied');
      assert.ok(result.movedCalls > result.stableCalls && result.movedCalls <= result.stableCalls + 2, 'changed geometry publishes a bounded new hit area');
      assert.ok(result.hidden && result.hiddenForCarry && result.disposed);
      assert.equal(result.afterHide, result.atHide, 'hide cancels pending geometry');
      assert.equal(result.afterDispose, result.atDispose, 'dispose cancels pending geometry');
      report.checks.push(`${edge}/${width}/${zoom}/${feedback}: fractional CSS, native style feedback, frame/timer liveness, changed anchor, hide, carry and disposal`);
      } catch (error) { report.failures.push({edge,width,zoom,feedback,message:error.message}); }
    }
    assert.deepEqual(report.errors, []);
    assert.deepEqual(report.network, []);
    assert.deepEqual(report.failures, [], `${report.failures.length} alert geometry cases failed; see report.json`);
    process.stdout.write(`${report.checks.length} focused alert geometry cases passed; no page errors or network requests.\n`);
  } finally {
    await browser.close();
    fs.writeFileSync(path.join(out, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  }
})().catch(error => { process.stderr.write(`${error.stack || error}\n`); process.exitCode = 1; });
