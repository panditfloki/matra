'use strict';

const { accountKey, maskIdentity, safePlan, epoch, finiteNumber } = require('./cursor-runtime');

function cadence(bucket) {
  for (const value of [bucket.window, bucket.bucketId, bucket.displayName]) {
    const text = typeof value === 'string' ? value.toLowerCase().replace(/_/g, '-') : '';
    if (/(^|[\s-])week(?:ly)?($|[\s-])/.test(text)) return 'weekly';
    if (/(^|[\s-])(session|5h|5-hour|five-hour|five hour|hourly)($|[\s-])/.test(text)) return 'session';
  }
  return null;
}

function family(value) {
  const text = String(value || '').toLowerCase();
  if (text.includes('gemini') || text.includes('flash')) return { id: 'gemini', label: 'Gemini' };
  if (/claude|gpt|openai/.test(text)) return { id: '3p', label: 'Claude + GPT' };
  return { id: 'quota', label: 'Antigravity' };
}

function remaining(bucket) {
  const direct = finiteNumber(bucket.remainingFraction);
  const nested = finiteNumber(bucket.remaining?.remainingFraction);
  const oneof = bucket.remaining?.case === 'remainingFraction' ? finiteNumber(bucket.remaining.value) : null;
  return direct ?? nested ?? oneof;
}

function usedPercent(bucket) {
  const left = remaining(bucket);
  if (left !== null && left >= 0 && left <= 1) return (1 - left) * 100;
  const used = finiteNumber(bucket.used);
  const limit = finiteNumber(bucket.limit);
  return used !== null && used >= 0 && limit !== null && limit > 0 ? used / limit * 100 : null;
}

function safeBucketLabel(value, fallback) {
  if (typeof value !== 'string') return fallback;
  const text = value.replace(/\s+(?:limit\s+)?remaining\s*$/i, '').trim();
  return text.length > 0 && text.length <= 80 && /^[\p{L}\p{N} .()+_-]+$/u.test(text) ? text : fallback;
}

function parseQuota(body) {
  if (!body || typeof body !== 'object' || Array.isArray(body)) return [];
  const groups = body.response?.groups ?? body.summary?.groups ?? body.groups ?? body.quotaGroups;
  const windows = [];
  const seen = new Map();
  function append(bucket, groupName, index, modelId) {
    if (!bucket || typeof bucket !== 'object' || bucket.disabled === true) return;
    const percent = usedPercent(bucket);
    if (percent === null) return;
    const group = family(groupName || modelId || bucket.modelId || bucket.name || bucket.bucketId);
    const cycle = cadence(bucket);
    const rawId = modelId || bucket.bucketId || bucket.modelId || bucket.name || String(index + 1);
    const suffix = String(rawId).toLowerCase().replace(/[^a-z0-9_-]+/g, '-').slice(0, 90);
    const baseId = `${group.id}-${suffix || index + 1}`;
    const duplicate = seen.get(baseId) || 0;
    seen.set(baseId, duplicate + 1);
    const label = cycle === 'weekly' ? 'Weekly limit' : cycle === 'session' ? '5-hour limit'
      : safeBucketLabel(bucket.displayName, 'Quota');
    windows.push({ id: duplicate ? `${baseId}-${duplicate + 1}` : baseId,
      label: `${group.label} · ${label}`, usedPercent: percent, resetsAt: epoch(bucket.resetTime) });
  }
  if (Array.isArray(groups) && groups.length) {
    const sorted = groups.slice(0, 16).sort((a, b) => Number(family(a?.displayName).id !== 'gemini')
      - Number(family(b?.displayName).id !== 'gemini'));
    for (const group of sorted) {
      const buckets = Array.isArray(group?.buckets) ? group.buckets.slice(0, 32) : [];
      buckets.sort((a, b) => (cadence(a) === 'weekly' ? 1 : 0) - (cadence(b) === 'weekly' ? 1 : 0));
      buckets.forEach((bucket, index) => { if (windows.length < 64) append(bucket, group.displayName, index); });
    }
  } else if (Array.isArray(body.buckets)) {
    // Older server envelopes provide model-specific buckets. Keep each bucket's
    // own measurement and reset; one model must never erase another window.
    body.buckets.slice(0, 64).forEach((bucket, index) => append(bucket, null, index));
  }
  return windows;
}

function parseStatus(body) {
  const status = body?.userStatus;
  if (!status || typeof status !== 'object' || Array.isArray(status)) return null;
  if (status.isSignedIn === false) return null;
  const email = typeof status.email === 'string' && /^[^\s@]+@[^\s@]+$/.test(status.email.trim())
    ? status.email.trim() : null;
  const tier = status.userTier;
  const info = status.planStatus?.planInfo;
  // The actual user tier takes precedence: planInfo can call an Ultra account Pro.
  const plan = [tier?.name, tier?.displayName, info?.planName, info?.planDisplayName,
    status.planName, status.plan, body.planName, body.plan].map(safePlan).find(Boolean) || null;
  const models = status.cascadeModelConfigData?.clientModelConfigs;
  const windows = [];
  if (Array.isArray(models)) {
    for (const [index, model] of models.slice(0, 64).entries()) {
      if (!model?.quotaInfo || model.disabled === true) continue;
      const percent = usedPercent(model.quotaInfo);
      if (percent === null) continue;
      const modelId = model.modelOrAlias?.model || model.modelId || model.label;
      const group = family(modelId || model.label);
      const suffix = String(modelId || index + 1).toLowerCase().replace(/[^a-z0-9_-]+/g, '-').slice(0, 90);
      windows.push({ id: `${group.id}-${suffix || index + 1}-${index + 1}`,
        label: `${group.label} · ${safeBucketLabel(model.label, 'Model quota')}`,
        usedPercent: percent, resetsAt: epoch(model.quotaInfo.resetTime) });
    }
  }
  return { key: email ? accountKey(email) : null, account: { label: maskIdentity(email), plan }, windows };
}

function headline(windows) {
  return windows.find(w => w.id.startsWith('gemini-') && !/weekly|week/i.test(w.label))?.id
    || windows.find(w => w.id.startsWith('gemini-'))?.id || windows[0]?.id || null;
}

module.exports = { parseQuota, parseStatus, headline, cadence, usedPercent };
