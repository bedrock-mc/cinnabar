const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');
const {validHistory, geometry} = require('../history.js');
const source = fs.readFileSync(path.join(__dirname, '../history.js'), 'utf8');

/** Supplies ordered observations with a real zero and a missing minute. */
function history(range = '24h') {
  return {range, interval_ms: 60000, from: 60000, to: 86460000, points: [
    {at: 60000, online: 0, peak: 0, samples: 1},
    {at: 120000, online: 8, peak: 8, samples: 1},
    {at: 240000, online: 4, peak: 4, samples: 1},
  ]};
}

test('keeps a zero baseline, missing-data gaps and the full time window', () => {
  const data = history();
  assert.equal(validHistory(data, '24h'), true);
  const chart = geometry(data, 600);
  assert.equal(chart.segments.length, 2);
  assert.equal(chart.segments[0][0].y, chart.bottom);
  assert.equal(chart.x(data.to), chart.right);
  assert.equal(chart.x(data.from), chart.left);
  assert.ok(chart.segments[1][0].x < chart.right / 2);
  assert.equal(validHistory({...data, points: []}, '24h'), true);
});

test('rejects bad ranges, timestamps, averages, weights, ordering and oversized payloads', () => {
  for (const change of [{range: '30d'}, {from: -1}, {to: 0}, {interval_ms: 0}, {points: null},
    {points: Array(1442).fill(history().points[0])},
    ...[{at: 61000}, {at: Infinity}, {online: NaN}, {online: -1}, {peak: -1}, {samples: 0}, {samples: 2}]
      .map(change => ({points: [{...history().points[0], ...change}]})),
    {points: [...history().points].reverse()}]) {
    assert.equal(validHistory({...history(), ...change}, '24h'), false);
  }
});

/** Creates the small DOM surface used by the chart and records its rendered SVG. */
function node() {
  return {textContent: '', hidden: false, clientWidth: 600, children: [], attributes: {}, events: {},
    setAttribute(name, value) { this.attributes[name] = value; },
    hasAttribute(name) { return Object.hasOwn(this.attributes, name); },
    toggleAttribute(name, force) { if (force) this.attributes[name] = ''; else delete this.attributes[name]; },
    appendChild(child) { this.children.push(child); },
    replaceChildren() { this.children = []; },
    addEventListener(name, callback) { this.events[name] = callback; },
    getBoundingClientRect() { return {left: 0}; }};
}

/** Starts the chart with deferred network replies so races and retries are observable. */
function page(hidden = false) {
  const elements = Object.fromEntries(['history-plot', 'history-message', 'history-summary', 'history-detail'].map(id => [id, node()]));
  elements['history-plot'].setAttribute('hidden', '');
  const buttons = ['24h', '7d', '30d'].map(range => Object.assign(node(), {dataset: {historyRange: range}}));
  const listeners = {}, requests = [], timers = new Map();
  const document = {visibilityState: hidden ? 'hidden' : 'visible',
    getElementById: id => elements[id], querySelectorAll: () => buttons,
    createElementNS: node, addEventListener: (name, callback) => { listeners[name] = callback; }};
  const window = {CINNABAR_DOWNLOADS: {online_history_url: 'https://api.invalid/v1/presence/history'},
    setTimeout(callback, delay) { const id = {}; timers.set(id, {callback, delay}); return id; },
    clearTimeout(id) { timers.delete(id); }};
  vm.runInNewContext(source, {document, window, URL, AbortController, AbortSignal, Intl,
    fetch(url, options) { return new Promise(resolve => requests.push({url, options, resolve})); }});
  return {elements, buttons, requests, timers, document, listeners,
    async reply(index, body, ok = true) {
      requests[index].resolve({ok, json: async () => body});
      await new Promise(resolve => setImmediate(resolve));
    }};
}

test('renders measured data, supports keyboard inspection, and retries after errors', async () => {
  const p = page();
  assert.equal(p.requests[0].url.searchParams.get('range'), '24h');
  assert.equal(p.requests[0].options.credentials, 'omit');
  assert.equal(p.requests[0].options.referrerPolicy, 'no-referrer');
  await p.reply(0, history());
  const plot = p.elements['history-plot'];
  assert.equal(plot.hasAttribute('hidden'), false);
  assert.match(p.elements['history-summary'].textContent, /Peak 8 · Average 4 · 3 sampled minutes/);
  assert.equal(plot.children.filter(child => child.attributes.class === 'chart-line').length, 1);
  plot.events.keydown({key: 'Home', preventDefault() {}});
  assert.match(p.elements['history-detail'].textContent, /0 players/);
  plot.events.keydown({key: 'ArrowRight', preventDefault() {}});
  assert.match(p.elements['history-detail'].textContent, /8 players/);
  plot.events.pointerdown({clientX: 32, pointerType: 'touch'});
  plot.events.focus();
  plot.events.pointerleave({pointerType: 'touch'});
  assert.match(p.elements['history-detail'].textContent, /0 players/);
  assert.equal([...p.timers.values()][0].delay, 60000);
  [...p.timers.values()][0].callback();
  await p.reply(1, null, false);
  assert.equal(plot.hasAttribute('hidden'), true);
  assert.equal(p.elements['history-summary'].textContent, '');
  assert.match(p.elements['history-message'].textContent, /unavailable/);
  [...p.timers.values()][0].callback();
  await p.reply(2, {...history(), points: []});
  assert.match(p.elements['history-message'].textContent, /No history yet/);
});

test('ignores obsolete responses on range changes and pauses requests in background tabs', async () => {
  const p = page(true);
  assert.equal(p.requests.length, 0);
  p.document.visibilityState = 'visible'; p.listeners.visibilitychange();
  p.buttons[1].events.click();
  assert.equal(p.requests[0].options.signal.aborted, true);
  await p.reply(1, history('7d'));
  await p.reply(0, {...history(), points: []});
  assert.equal(p.elements['history-plot'].hasAttribute('hidden'), false);
  assert.equal(p.buttons[1].attributes['aria-pressed'], 'true');
  p.document.visibilityState = 'hidden'; p.listeners.visibilitychange();
  assert.equal(p.timers.size, 0);
  p.document.visibilityState = 'visible'; p.listeners.visibilitychange();
  assert.equal(p.requests.length, 3);
});
