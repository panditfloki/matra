'use strict';

const MAX_BYTES = 512 * 1024;

function abortable(operation, signal) {
  if (!signal) return operation;
  if (signal.aborted) {
    Promise.resolve(operation).catch(() => {});
    return Promise.reject(new Error('usage request expired'));
  }
  return new Promise((resolve, reject) => {
    const abort = () => reject(new Error('usage request expired'));
    signal.addEventListener('abort', abort, { once: true });
    operation.then(resolve, reject).finally(() => signal.removeEventListener('abort', abort));
  });
}

function cancel(reader) {
  try { Promise.resolve(reader.cancel()).catch(() => {}); } catch { /* already closed */ }
}

async function boundedJson(response, signal) {
  if (response.body?.getReader) {
    const reader = response.body.getReader();
    const chunks = [];
    let bytes = 0;
    try {
      const declared = response.headers?.get?.('content-length');
      if (typeof declared === 'string' && /^\d+$/.test(declared) && Number(declared) > MAX_BYTES) throw new Error('usage response too large');
      while (true) {
        const { done, value } = await abortable(reader.read(), signal);
        if (done) break;
        const length = typeof value?.byteLength === 'number' ? value.byteLength : typeof value === 'string' ? Buffer.byteLength(value) : null;
        if (length === null) throw new Error('invalid usage response');
        bytes += length;
        if (bytes > MAX_BYTES) throw new Error('usage response too large');
        const chunk = Buffer.from(value);
        chunks.push(chunk);
      }
      return JSON.parse(Buffer.concat(chunks, bytes).toString('utf8'));
    } catch (error) {
      cancel(reader);
      throw error;
    } finally {
      try { reader.releaseLock(); } catch { /* pending read after abort */ }
    }
  }
  // Injected fixture responses may expose only json(). Real fetch responses
  // always use the streaming branch above, with the timeout through body read.
  const value = await abortable(Promise.resolve().then(() => response.json()), signal);
  if (Buffer.byteLength(JSON.stringify(value) || '', 'utf8') > MAX_BYTES) throw new Error('usage response too large');
  return value;
}

module.exports = { boundedJson };
