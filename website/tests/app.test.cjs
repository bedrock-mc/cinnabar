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
      setAttribute(name, value) { this.attrs[name] = value; },
      contains(target) { return target === this || (this.children || []).includes(target); },
      attrs: {},
    });
    return elements.get(id);
  }
  const channelIds = {'channel-beta': 'beta', 'channel-nightly': 'nightly'};
  for (const [id, channel] of Object.entries(channelIds)) {
    Object.assign(element(id), {dataset: {channel}, attrs: {}});
  }
  element('channel-menu').hidden = true;
  element('channel-toggle').attrs = {};
  element('download-split').children = [element('channel-toggle'), element('channel-menu'), ...Object.keys(channelIds).map(element)];
  const outside = {};
  const documentListeners = {};
  const links = Object.entries(config.assets).flatMap(([os, arches]) =>
    Object.keys(arches).map(arch => ({dataset: {releaseAsset: `${os}/${arch}`}})));
  const window = {CINNABAR_DOWNLOADS: config, location: {assign(url) { this.destination = url; }}};
  const navigator = {userAgent, platform, maxTouchPoints: touchPoints,
    clipboard: {writeText: async text => { navigator.copied = text; }}};
  vm.runInNewContext(source, {window, navigator, document: {
    getElementById: element, querySelectorAll: () => links,
    addEventListener(name, callback) { documentListeners[name] = callback; },
  }});
  const key = (id, name) => {
    const event = {key: name, target: element(id), prevented: false, preventDefault() { this.prevented = true; }};
    element(id.startsWith('channel-t') ? id : 'channel-menu').listeners.keydown(event);
    return event;
  };
  return {element, window, navigator, links, outside, key, documentClick: target => documentListeners.click({target}), click: id => element(id).listeners.click({currentTarget: element(id)})};
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

const nightlyAsset = (os, arch) => config.nightly_base + config.assets[os][arch];

test('nightly URLs derive from the repository and manifest assets', () => {
  const nightly = new URL(config.nightly_base);
  assert.equal(nightly.pathname, `/${config.repository}/releases/download/nightly/`);
  assert.equal(config.nightly_install_command, `${config.install_command} -s -- --channel nightly`);
});

test('Beta stays the default for the main button and dialog links', () => {
  const p = page('Windows NT 10.0', 'Win32');
  p.click('download');
  assert.equal(p.window.location.destination, config.download_base + config.assets.windows.x86_64);
  assert.equal(p.element('channel-beta').attrs['aria-checked'], 'true');
  assert.equal(p.element('channel-menu').hidden, true);
});

test('choosing Nightly downloads the nightly asset on Windows and keeps it selected', () => {
  const p = page('Windows NT 10.0', 'Win32');
  p.click('channel-toggle');
  assert.equal(p.element('channel-menu').hidden, false);
  assert.equal(p.element('channel-toggle').attrs['aria-expanded'], 'true');
  p.click('channel-nightly');
  assert.equal(p.window.location.destination, nightlyAsset('windows', 'x86_64'));
  assert.equal(p.element('channel-nightly').attrs['aria-checked'], 'true');
  assert.equal(p.element('channel-beta').attrs['aria-checked'], 'false');
  assert.equal(p.element('channel-menu').hidden, true);
  p.click('download');
  assert.equal(p.window.location.destination, nightlyAsset('windows', 'x86_64'));
});

test('Nightly on Mac and Linux updates the dialog links and install command', () => {
  const mac = page('Macintosh', 'MacIntel');
  mac.click('channel-nightly');
  assert.equal(mac.element('macos-install').hidden, false);
  for (const link of mac.links) {
    const [os, arch] = link.dataset.releaseAsset.split('/');
    assert.equal(link.href, nightlyAsset(os, arch));
  }
  const linux = page('X11; Linux x86_64', 'Linux x86_64');
  linux.click('channel-nightly');
  assert.equal(linux.element('linux-install').hidden, false);
  assert.equal(linux.element('install-command').value, config.nightly_install_command);
  linux.click('channel-beta');
  assert.equal(linux.element('install-command').value, config.install_command);
});

test('copy follows the active channel', async () => {
  const p = page('Linux', 'Linux');
  p.click('channel-nightly');
  await p.click('copy-command');
  assert.equal(p.navigator.copied, config.nightly_install_command);
});

test('menu keyboard: arrows move focus, Space selects, Escape returns to the caret', () => {
  const p = page('Windows NT 10.0', 'Win32');
  assert.equal(p.key('channel-toggle', 'ArrowDown').prevented, true);
  assert.equal(p.element('channel-menu').hidden, false);
  assert.equal(p.element('channel-beta').focused, true);
  p.element('channel-beta').focused = false;
  p.key('channel-beta', 'ArrowDown');
  assert.equal(p.element('channel-nightly').focused, true);
  p.element('channel-beta').focused = false;
  p.key('channel-nightly', 'ArrowDown');
  assert.equal(p.element('channel-beta').focused, true);
  p.element('channel-toggle').focused = false;
  p.key('channel-beta', 'Escape');
  assert.equal(p.element('channel-menu').hidden, true);
  assert.equal(p.element('channel-toggle').attrs['aria-expanded'], 'false');
  assert.equal(p.element('channel-toggle').focused, true);
  p.key('channel-toggle', 'ArrowUp');
  assert.equal(p.element('channel-nightly').focused, true);
  p.key('channel-nightly', ' ');
  assert.equal(p.window.location.destination, nightlyAsset('windows', 'x86_64'));
  assert.equal(p.element('channel-menu').hidden, true);
});

test('Enter selects the focused channel and outside clicks close the menu', () => {
  const p = page('Windows NT 10.0', 'Win32');
  p.click('channel-toggle');
  p.documentClick(p.element('channel-nightly'));
  assert.equal(p.element('channel-menu').hidden, false);
  p.documentClick(p.outside);
  assert.equal(p.element('channel-menu').hidden, true);
  p.click('channel-toggle');
  p.key('channel-nightly', 'Enter');
  assert.equal(p.window.location.destination, nightlyAsset('windows', 'x86_64'));
});
