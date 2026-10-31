import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, readdir, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import bridge from '../Sources/Resources/matra-opencode.mjs'

test('bridge keeps only metadata, hides unmapped helpers, and suppresses false completion', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'matra-bridge-'))
  const previous = process.env.MATRA_ACTIVITY_DIR
  process.env.MATRA_ACTIVITY_DIR = directory
  let parentKnown = false
  const plugin = await bridge({ client: { session: {
    get: async ({ path }) => ({ data: path.id === 'helper' && !parentKnown ? null : {
      title: 'SECRET_PROMPT_TITLE', parentID: path.id === 'helper' ? 'root' : null,
    } }),
    messages: async () => ({ data: [{ info: { providerID: 'local', modelID: 'model-a' },
      parts: [{ text: 'SECRET_ANSWER' }] }] }),
  } } })
  const send = (type, properties) => plugin.event({ event: { type, properties } })
  const read = async () => {
    const files = (await readdir(directory)).filter(name => name.endsWith('.json'))
    assert.equal(files.length, 1)
    return JSON.parse(await readFile(join(directory, files[0]), 'utf8'))
  }
  try {
    await send('session.status', { sessionID: 'root', status: { type: 'busy' } })
    await send('session.updated', { info: { id: 'root', title: 'SECRET_PROMPT_TITLE' } })
    await send('message.updated', { info: { sessionID: 'helper', providerID: 'local', modelID: 'model-a' } })
    let data = await read()
    assert.ok(data.events.filter(e => e.session.sessionID === 'helper')
      .every(e => e.session.providerID === ''))
    parentKnown = true
    await send('session.status', { sessionID: 'helper', status: { type: 'busy' } })
    data = await read()
    assert.equal(data.sessions.find(s => s.sessionID === 'helper').parentID, 'root')
    await send('permission.asked', { sessionID: 'root', id: 'permission-1', args: 'SECRET_TOOL_ARGS' })
    assert.equal((await read()).sessions.find(s => s.sessionID === 'root').state, 'waiting')
    await send('permission.replied', { sessionID: 'root', permissionID: 'permission-1', reply: 'SECRET_ANSWER' })
    assert.equal((await read()).sessions.find(s => s.sessionID === 'root').state, 'busy')
    await send('message.updated', { info: { sessionID: 'root', error: 'SECRET_ERROR', content: 'SECRET_PROMPT' } })
    await send('session.status', { sessionID: 'root', status: { type: 'idle' } })
    data = await read()
    assert.equal(data.events.filter(e => e.session.sessionID === 'root').at(-1).session.state, 'ended')
    assert.ok(!JSON.stringify(data).includes('SECRET_'))
    await send('session.deleted', { info: { id: 'helper' } })
    assert.equal((await read()).sessions.length, 0)
  } finally {
    await plugin.dispose()
    assert.equal((await readdir(directory)).length, 0)
    if (previous === undefined) delete process.env.MATRA_ACTIVITY_DIR
    else process.env.MATRA_ACTIVITY_DIR = previous
    await rm(directory, { recursive: true })
  }
})
