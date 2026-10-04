'use strict';

const path = require('node:path');
const { runFile, failure, MAX_FILE } = require('./cursor-runtime');

// mode=ro, never immutable: an editor's latest rotated token may exist only in WAL.
// The Python fallback uses only its stdlib and never installs a module.
const PYTHON_QUERY = [
  'import sqlite3,json,sys,pathlib',
  'filename,keys=sys.argv[1],json.loads(sys.argv[2])',
  'db=sqlite3.connect(pathlib.Path(filename).resolve().as_uri()+"?mode=ro",uri=True,timeout=0.25)',
  'db.execute("PRAGMA query_only=ON")',
  'sql="SELECT key,substr(value,1,262145) FROM ItemTable WHERE key IN ("+",".join("?" for _ in keys)+") LIMIT 32"',
  'rows=[{"key":k,"value":v.decode("utf-8") if isinstance(v,bytes) else v} for k,v in db.execute(sql,keys)]',
  'db.close()',
  'print(json.dumps(rows))'
].join('\n');

function checkedRows(rows, keys) {
  if (!Array.isArray(rows) || rows.length > 32) throw failure('database-failed');
  const result = Object.create(null);
  for (const row of rows) {
    if (!keys.includes(row.key)) continue;
    const value = Buffer.isBuffer(row.value) ? row.value.toString('utf8') : row.value;
    if (typeof value === 'string') {
      if (Buffer.byteLength(value) > MAX_FILE) throw failure('database-failed');
      result[row.key] = value;
    }
  }
  return result;
}

async function readItemTable(file, keys, options = {}) {
  if (typeof file !== 'string' || !path.isAbsolute(file)
      || !Array.isArray(keys) || !keys.length || keys.length > 32
      || keys.some(key => typeof key !== 'string' || !/^[a-zA-Z0-9/_-]{1,120}$/.test(key))) {
    throw failure('database-failed');
  }

  // Test injection disables the in-process driver so process-fallback branches remain testable.
  if (!options.execFile) {
    let DatabaseSync;
    try { ({ DatabaseSync } = require('node:sqlite')); } catch { }
    if (DatabaseSync) {
      let db;
      try {
        db = new DatabaseSync(file, { readOnly: true, timeout: 250 });
        db.exec('PRAGMA query_only=ON; PRAGMA busy_timeout=250;');
        const placeholders = keys.map(() => '?').join(',');
        const rows = db.prepare(`SELECT key, substr(value, 1, 262145) AS value FROM ItemTable WHERE key IN (${placeholders}) LIMIT 32`).all(...keys);
        return checkedRows(rows, keys);
      } catch { throw failure('database-failed'); }
      finally { try { db?.close(); } catch { } }
    }
  }

  let missing = true;
  // Keys are a fixed provider-owned allowlist, validated above. Database paths travel as argv.
  const sql = `PRAGMA query_only=ON; SELECT key, substr(value, 1, 262145) AS value FROM ItemTable WHERE key IN (${keys.map(key => `'${key}'`).join(',')}) LIMIT 32;`;
  const sqlite = (options.platform || process.platform) === 'darwin' ? '/usr/bin/sqlite3' : 'sqlite3';
  try {
    const output = await runFile(options.execFile, sqlite, ['-readonly', '-json', '-cmd', '.timeout 250', file, sql]);
    const tail = output.trim();
    return checkedRows(tail ? JSON.parse(tail) : [], keys);
  } catch (error) {
    if (error.code !== 'missing-command') missing = false;
  }

  const candidates = (options.platform || process.platform) === 'win32'
    ? [['py', ['-3']], ['python3', []], ['python', []]] : [['python3', []]];
  for (const [python, prefix] of candidates) {
    try {
      const output = await runFile(options.execFile, python, [...prefix, '-c', PYTHON_QUERY, file, JSON.stringify(keys)]);
      return checkedRows(JSON.parse(output), keys);
    } catch (error) {
      if (error.code !== 'missing-command') missing = false;
    }
  }
  throw failure(missing ? 'sqlite-unavailable' : 'database-failed');
}

module.exports = { readItemTable, PYTHON_QUERY };
