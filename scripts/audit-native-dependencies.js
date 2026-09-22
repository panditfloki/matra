const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const root = path.join(__dirname, '..');
const cacheRoot = path.join(os.homedir(), '.cargo', 'registry', 'cache');
const caches = fs.readdirSync(cacheRoot).map(x => path.join(cacheRoot, x));
const packages = fs.readFileSync(path.join(root, 'native', 'Cargo.lock'), 'utf8').split('[[package]]')
  .filter(x => x.includes('source = "registry')).map(x => ({ name: /^name = "([^"]+)"/m.exec(x)[1], version: /^version = "([^"]+)"/m.exec(x)[1] }))
  .filter(p => !caches.some(c => fs.existsSync(path.join(c, `${p.name}-${p.version}.crate`))));
let index = 0; const results = [];
async function worker() {
  while (index < packages.length) {
    const p = packages[index++];
    try {
      const res = await fetch(`https://crates.io/api/v1/crates/${p.name}/${p.version}`, { headers: { 'User-Agent': 'Matra-local-dependency-audit/1.0' }, signal: AbortSignal.timeout(20000) });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json(); const published = data.version.created_at;
      const age = Math.floor((new Date('2026-09-22T00:00:00Z') - new Date(published)) / 86400000);
      results.push({ ...p, published, age });
    } catch (error) { results.push({ ...p, error: error.message }); }
  }
}
Promise.all(Array.from({ length: 4 }, worker)).then(() => {
  fs.mkdirSync(path.join(root, '.desktop-test'), { recursive: true });
  fs.writeFileSync(path.join(root, '.desktop-test', 'native-dependency-audit.json'), JSON.stringify(results, null, 2));
  console.log(JSON.stringify({ checked: results.length, young: results.filter(p => p.age < 14), failed: results.filter(p => p.error) }, null, 2));
});
