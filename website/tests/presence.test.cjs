const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');

const source = fs.readFileSync(path.join(__dirname, '../presence.js'), 'utf8');

/** Runs presence updates with controlled responses, visibility and scheduled callbacks. */
async function page(reply = {online: 42}, hidden = false) {
  const label = {textContent: ''};
  const listeners = {};
  const timers = new Map();
  const requests = [];
  let response = reply;
  let timerId = 0;
  const document = {hidden, getElementById: () => label,
    addEventListener: (name, callback) => { listeners[name] = callback; }};
  const window = {CINNABAR_DOWNLOADS: {online_count_url: 'https://api.invalid/v1/presence/count'},
    setTimeout(callback, delay) { timers.set(++timerId, {callback, delay}); return timerId; },
    clearTimeout(id) { timers.delete(id); }};
  vm.runInNewContext(source, {document, window, AbortSignal,
    async fetch(url, options) {
      requests.push({url, options});
      if (response instanceof Error) throw response;
      return {ok: response !== 'http-error', json: async () => response};
    }});
  const settle = () => new Promise(resolve => setImmediate(resolve));
  await settle();
  return {label, requests, timers,
    async refresh(next) {
      response = next;
      const timer = timers.values().next().value;
      assert.ok(timer);
      timers.clear();
      await timer.callback();
    },
    async visible(value) {
      document.hidden = !value;
      listeners.visibilitychange();
      await settle();
    }};
}

test('shows live counts with singular, plural, zero and formatted numbers', async () => {
  const p = await page();
  assert.equal(p.label.textContent, '42 people are using Cinnabar right now');
  assert.equal(p.requests[0].url, 'https://api.invalid/v1/presence/count');
  assert.equal(p.requests[0].options.credentials, 'omit');
  assert.equal(p.requests[0].options.referrerPolicy, 'no-referrer');
  assert.ok(p.requests[0].options.signal instanceof AbortSignal);
  assert.equal([...p.timers.values()][0].delay, 60_000);
  await p.refresh({online: 1});
  assert.equal(p.label.textContent, '1 person is using Cinnabar right now');
  await p.refresh({online: 0});
  assert.equal(p.label.textContent, '0 people are using Cinnabar right now');
  await p.refresh({online: 12345});
  assert.equal(p.label.textContent, `${(12345).toLocaleString()} people are using Cinnabar right now`);
});

test('clears stale counts on errors or invalid payloads and recovers on the next poll', async () => {
  for (const invalid of [new Error('offline'), new DOMException('timeout', 'TimeoutError'),
    'http-error', null, {}, {online: -1}, {online: 1.5}, {online: '2'},
    {online: Number.MAX_SAFE_INTEGER + 1}, {online: '<script>'}]) {
    const p = await page();
    await p.refresh(invalid);
    assert.equal(p.label.textContent, '');
    await p.refresh({online: 2});
    assert.equal(p.label.textContent, '2 people are using Cinnabar right now');
  }
});

test('pauses polling in background tabs and refreshes on return', async () => {
  const p = await page({online: 2}, true);
  assert.equal(p.requests.length, 0);
  assert.equal(p.timers.size, 0);
  await p.visible(true);
  assert.equal(p.requests.length, 1);
  assert.equal(p.timers.size, 1);
  await p.visible(false);
  assert.equal(p.label.textContent, '');
  assert.equal(p.timers.size, 0);
  await p.visible(true);
  assert.equal(p.requests.length, 2);
  assert.equal(p.label.textContent, '2 people are using Cinnabar right now');
});
