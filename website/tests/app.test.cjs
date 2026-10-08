const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');

const root = path.resolve(__dirname, '..');
const output = process.env.CINNABAR_SITE_OUTPUT || path.join(root, 'dist');
const context = {window: {}};
vm.runInNewContext(fs.readFileSync(path.join(output, 'downloads.js'), 'utf8'), context);
const config = context.window.CINNABAR_DOWNLOADS;
const source = fs.readFileSync(path.join(root, 'app.js'), 'utf8');

test('Linux command fetches the installer from the latest GitHub release', () => {
  assert.equal(config.install_command, `curl -fsSL ${config.download_base}${config.install_script} | sh`);
  const release = new URL(config.download_base);
  assert.equal(release.protocol, 'https:');
  assert.equal(release.pathname, `/${config.repository}/releases/latest/download/`);
});

function page(userAgent, platform, touchPoints = 0) {
  const elements = new Map();
  function element(id) {
    if (!elements.has(id)) elements.set(id, {
      textContent: '', hidden: false, listeners: {},
      addEventListener(name, callback) { this.listeners[name] = callback; },
      showModal() { this.open = true; }, close() { this.open = false; },
      focus() { this.focused = true; }, select() { this.selected = true; },
    });
    return elements.get(id);
  }
  const links = Object.entries(config.assets).flatMap(([os, arches]) =>
    Object.keys(arches).map(arch => ({dataset: {releaseAsset: `${os}/${arch}`}})));
  const window = {CINNABAR_DOWNLOADS: config, location: {assign(url) { this.destination = url; }}};
  const navigator = {userAgent, platform, maxTouchPoints: touchPoints,
    clipboard: {writeText: async text => { navigator.copied = text; }}};
  vm.runInNewContext(source, {window, navigator, document: {
    getElementById: element, querySelectorAll: () => links,
  }});
  return {element, window, navigator, links, click: id => element(id).listeners.click({currentTarget: element(id)})};
}

test('Linux shows a copyable installer command without navigating', () => {
  const p = page('X11; Linux x86_64', 'Linux x86_64');
  p.click('download');
  assert.equal(p.element('install-dialog').open, true);
  assert.equal(p.element('linux-install').hidden, false);
  assert.equal(p.element('macos-install').hidden, true);
  assert.equal(p.element('install-command').value, config.install_command);
  assert.equal(p.window.location.destination, undefined);
  p.click('close-dialog');
  assert.equal(p.element('install-dialog').open, false);
});

test('Windows downloads the manifest setup executable', () => {
  const p = page('Windows NT 10.0', 'Win32');
  p.click('download');
  assert.equal(p.window.location.destination, config.download_base + config.assets.windows.x86_64);
  assert.equal(p.element('install-dialog').open, undefined);
});

test('Mac users get separate Apple silicon and Intel disk images', () => {
  const p = page('Macintosh', 'MacIntel');
  p.click('download');
  assert.equal(p.element('macos-install').hidden, false);
  for (const link of p.links) {
    const [os, arch] = link.dataset.releaseAsset.split('/');
    assert.equal(link.href, config.download_base + config.assets[os][arch]);
  }
});

test('mobile, iPad desktop agents and ChromeOS offer desktop choices', () => {
  for (const [agent, platform, touch] of [['Linux; Android', 'Linux', 5], ['iPhone', 'iPhone', 5],
    ['Macintosh', 'MacIntel', 5], ['X11; CrOS', 'Linux', 0], ['unknown', '', 0]]) {
    const p = page(agent, platform, touch);
    p.click('download');
    assert.equal(p.element('desktop-install').hidden, false);
    assert.equal(p.element('download-label').textContent, 'Desktop downloads');
  }
});

test('copy completes after the browser clears event.currentTarget', async () => {
  const p = page('Linux', 'Linux');
  const event = {currentTarget: p.element('copy-command')};
  const pending = p.element('copy-command').listeners.click(event);
  event.currentTarget = null;
  await pending;
  assert.equal(p.navigator.copied, config.install_command);
  assert.equal(p.element('copy-command').textContent, 'Copied');
});

test('clipboard rejection selects the install command for manual copying', async () => {
  const p = page('Linux', 'Linux');
  p.navigator.clipboard.writeText = async () => { throw new Error('Clipboard unavailable'); };
  await p.click('copy-command');
  assert.equal(p.element('install-command').focused, true);
  assert.equal(p.element('install-command').selected, true);
});
