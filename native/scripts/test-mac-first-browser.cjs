// Real Chromium rendering of the shipped Windows HTML, with an inert sample bridge.
// This does not certify WebView2, native hit-testing, drag or Mac visual acceptance.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { pathToFileURL } = require('node:url');
const { execFileSync } = require('node:child_process');
const { probeHandles, assertHandles } = require('../../scripts/native-handle-test.js');
const puppeteer = require(process.env.MATRA_PUPPETEER_PATH || 'puppeteer');

const repo = path.resolve(__dirname, '../..');
const task = path.resolve(repo, '..');
const baseline = process.argv.includes('--baseline');
const out = path.join(task, 'evidence', baseline ? 'browser-baseline' : 'browser-current');
const review = path.join(task, 'artifacts', baseline ? 'baseline-notch-review' : 'notch-review');
fs.mkdirSync(out, { recursive: true });
const contained = p => { const resolved = fs.realpathSync(p); assert.ok(resolved.startsWith(fs.realpathSync(task) + path.sep), 'Output must stay inside task: ' + resolved); };
contained(out);
execFileSync(process.execPath, [path.join(repo, 'scripts/build-notch-review.mjs'), review, ...(baseline ? ['--baseline'] : [])], { stdio: 'pipe' });
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));

(async () => {
  const browser = await puppeteer.launch({
    executablePath: process.env.MATRA_CHROME_PATH,
    headless: true,
    userDataDir: path.join(task, 'cache', baseline ? 'browser-baseline-profile' : 'browser-profile'),
    env: { ...process.env, TMPDIR: path.join(task, 'tmp') },
    args: ['--disable-background-networking', '--disable-component-update', '--no-first-run', '--no-default-browser-check', '--disable-crash-reporter', '--disable-breakpad', '--disable-sync', '--disable-features=MediaRouter']
  });
  const checks = [], stressChecks = [], errors = [], network = [];
  try {
    const page = await browser.newPage();
    page.on('pageerror', error => errors.push(error.message));
    await page.setRequestInterception(true);
    page.on('request', request => {
      if (/^https?:/.test(request.url())) { network.push(request.url()); return request.abort(); }
      return request.continue();
    });
    await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'reduce' }]);
    for (const scale of baseline ? [1] : [.8, 1, 1.25]) for (const dpr of baseline ? [1] : [1, 1.5]) for (const edge of ['right', 'left', 'top', 'bottom']) for (const count of [1, 3, 6]) {
      const vertical = edge === 'right' || edge === 'left';
      await page.setViewport({ width: vertical ? 460 : 650, height: 650, deviceScaleFactor: dpr * scale });
      await page.goto(pathToFileURL(path.join(review, 'windows-notch-component.html')).href + '?scale=' + scale);
      await page.waitForFunction(() => typeof window.__matraSample === 'object' && document.querySelector('.cell'));
      await page.evaluate(({ edge, count }) => { window.__matraSample.configure({ edge, count }); document.documentElement.style.background = '#596976'; }, { edge, count });
      await pause(120);
      const geometry = await page.evaluate(() => {
        const box = selector => { const r = document.querySelector(selector).getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height, right: r.right, bottom: r.bottom }; };
        return { pill: box('#pill'), ring: box('.ringwrap'), orb: box('#orb'), move: box('#move'), rest: box('#rest'), count: document.querySelectorAll('.cell').length, width: innerWidth, height: innerHeight, zoom: parseFloat(document.documentElement.style.zoom) || 1, classes: document.body.className };
      });
      assert.equal(geometry.count, count, `${edge}/${count}/${dpr}: fixture provider count`);
      const p = geometry.pill;
      assert.ok(p.width > 0 && p.height > 0 && p.x >= -1 && p.y >= -1 && p.right <= geometry.width + 1 && p.bottom <= geometry.height + 1, `${edge}/${count}/${dpr}: capsule fits viewport ${JSON.stringify(geometry)}`);
      assert.ok(Math.abs(geometry.ring.width / geometry.zoom - 44) < .2, `${edge}/${count}/${dpr}: original Mac 44pt ring anchor`);
      await page.hover('.cell');
      await page.waitForFunction(() => document.querySelector('#card').classList.contains('show'));
      const card = await page.evaluate(() => {
        const c = document.querySelector('#card'), r = c.getBoundingClientRect();
        return { x: r.x, y: r.y, width: r.width, height: r.height, right: r.right, bottom: r.bottom, text: c.textContent, overflow: c.scrollHeight > c.clientHeight };
      });
      assert.ok(card.width > 0 && card.height > 0 && card.x >= -1 && card.y >= -1 && card.right <= geometry.width + 1 && card.bottom <= geometry.height + 1, `${edge}/${count}/${dpr}: hover card fits viewport ${JSON.stringify(card)}`);
      assert.ok(card.text.includes('Claude'), 'Sample provider card renders actual content');
      if (!baseline) assert.ok(Math.abs(card.width * scale - 600 * 44 / 117) < 1, `${edge}/${count}/${dpr}/${scale}: card retains the Mac width independently of notch size`);
      if (count === 3 && dpr === 1 && scale === 1) await page.screenshot({ path: path.join(out, `${edge}-hover.png`) });
      await page.click('.cell');
      assert.ok(await page.evaluate(() => window.__matraSample.calls.some(c => c.name === 'refresh_ring' && c.args.provider === 'claude')), 'Provider click reaches the native refresh command boundary');
      checks.push({ edge, count, dpr, scale, geometry, card });
    }
    if (!baseline) for (const scale of [.8, 1, 1.25]) for (const edge of ['right', 'left', 'top', 'bottom']) {
      const vertical = ['right', 'left'].includes(edge);
      const insets = vertical ? [0, 0, 60, 0] : [0, 60, 0, 0];
      await page.setViewport({ width: vertical ? 460 : 650, height: 650, deviceScaleFactor: 1.5 * scale });
      await page.goto(pathToFileURL(path.join(review, 'windows-notch-component.html')).href + '?scale=' + scale);
      await page.waitForFunction(() => window.__matraSample && document.querySelector('.cell'));
      await page.evaluate(options => window.__matraSample.configure(options), { edge, count: 10, insets });
      await pause(120);
      const before = await page.$eval('.cell', el => ({ width: el.getBoundingClientRect().width, height: el.getBoundingClientRect().height }));
      await page.evaluate(options => window.__matraSample.configure(options), { edge, count: 10, insets, readings: false });
      await pause(120);
      const after = await page.$eval('.cell', el => ({ width: el.getBoundingClientRect().width, height: el.getBoundingClientRect().height }));
      assert.ok(Math.abs(before.width - after.width) < .2 && Math.abs(before.height - after.height) < .2, `${edge}/${scale}: hiding readings preserves the Mac provider cell box: ${JSON.stringify({ before, after })}`);
      await page.hover('#pill');
      await page.mouse.wheel({ deltaY: 2000 });
      await pause(150);
      const scrolled = await page.$eval('#pill', el => ({ top: el.scrollTop, left: el.scrollLeft, maxX: el.scrollWidth - el.clientWidth, maxY: el.scrollHeight - el.clientHeight }));
      assert.ok(vertical ? scrolled.top > 0 : scrolled.left > 0, `${edge}/${scale}: an ordinary mouse wheel reaches overflowed providers`);
      const last = '.cell:last-child';
      await page.hover(last);
      await page.waitForFunction(() => document.querySelector('#card').textContent.includes('Sample provider 10') && document.querySelector('#card').classList.contains('show'));
      await page.click(last);
      assert.ok(await page.evaluate(() => window.__matraSample.calls.some(c => c.name === 'refresh_ring' && c.args.provider === 'custom:sample-4')), 'Last overflowed provider can be refreshed');
      const controls = await page.evaluate(() => ['#pill', '#orb', '#move'].map(selector => {
        const r = document.querySelector(selector).getBoundingClientRect(); return { selector, left: r.left, top: r.top, right: r.right, bottom: r.bottom, w: innerWidth, h: innerHeight, z: parseFloat(document.documentElement.style.zoom) || 1 };
      }));
      for (const r of controls) assert.ok(r.left >= -1 && r.top >= -1 && r.right <= r.w - insets[1] * r.z + 1 && r.bottom <= r.h - insets[2] * r.z + 1, `${edge}/${scale}: ${r.selector} fits with taskbar inset ${JSON.stringify(r)}`);
      const handles = await page.evaluate(probeHandles);
      assertHandles(handles, `${edge}/${scale}: browser control contract`);
      stressChecks.push({ edge, scale, insets, before, after, scrolled, controls, handles });
    }
    // Exercise the standalone review itself, including its iframe and controls.
    await page.setViewport({ width: 1100, height: 950, deviceScaleFactor: 1 });
    await page.goto(pathToFileURL(path.join(review, 'index.html')).href);
    await page.waitForFunction(() => document.querySelector('iframe').contentWindow.__matraSample?.calls.length > 0);
    for (const edge of ['left', 'top', 'right', 'bottom']) {
      await page.select('#edge', edge);
      assert.equal(await page.evaluate(() => document.querySelector('iframe').contentDocument.body.dataset.edge), edge);
    }
    await page.select('#edge', 'right'); await page.select('#theme', 'light'); await pause(150);
    assert.equal(await page.evaluate(() => document.querySelector('iframe').contentDocument.documentElement.dataset.matraTheme), 'light');
    await page.screenshot({ path: path.join(out, 'review-light.png') });
    await page.select('#theme', 'dark'); await page.screenshot({ path: path.join(out, 'review-dark.png') });
    assert.deepEqual(network, [], 'Review must not request network resources');
    assert.deepEqual(errors, [], 'No JavaScript runtime errors');
    fs.writeFileSync(path.join(out, 'result.json'), JSON.stringify({ passed: true, browser: await browser.version(), baseline, sampleData: true, nativeAcceptance: false, checks, stressChecks, errors, network }, null, 2) + '\n');
    console.log(JSON.stringify({ passed: true, cases: checks.length, stressCases: stressChecks.length, evidence: out, nativeAcceptance: false }));
  } catch (error) {
    fs.writeFileSync(path.join(out, 'failure.json'), JSON.stringify({ message: error.message, stack: error.stack, checks, stressChecks, errors, network }, null, 2) + '\n');
    throw error;
  } finally { await browser.close(); }
})().catch(error => { console.error(error.stack); process.exitCode = 1; });
