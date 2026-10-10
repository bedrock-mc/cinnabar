function detectPlatform(userAgent, platform, touchPoints) {
  if (/Android|iPhone|iPad|iPod/i.test(userAgent) || (/Mac/i.test(platform) && touchPoints > 1)) return 'mobile';
  if (/Windows|Win/i.test(platform + ' ' + userAgent)) return 'windows';
  if (/CrOS/i.test(userAgent)) return 'unsupported';
  if (/Mac/i.test(platform + ' ' + userAgent)) return 'macos';
  if (/Linux/i.test(platform + ' ' + userAgent)) return 'linux';
  return 'unsupported';
}

const config = window.CINNABAR_DOWNLOADS;
const platform = detectPlatform(navigator.userAgent, navigator.userAgentData?.platform || navigator.platform, navigator.maxTouchPoints);
const labels = {windows: 'Windows', macos: 'macOS', linux: 'Linux'};
const button = document.getElementById('download');
const dialog = document.getElementById('install-dialog');
document.getElementById('download-label').textContent = labels[platform] ? `Download for ${labels[platform]}` : 'Desktop downloads';
const split = document.getElementById('download-split');
const toggle = document.getElementById('channel-toggle');
const menu = document.getElementById('channel-menu');
const options = [document.getElementById('channel-beta'), document.getElementById('channel-nightly')];
let channel = 'beta';
const base = () => channel === 'nightly' ? config.nightly_base : config.download_base;
const installCommand = () => channel === 'nightly' ? config.nightly_install_command : config.install_command;

function applyChannel() {
  document.getElementById('install-command').value = installCommand();
  document.querySelectorAll('[data-release-asset]').forEach(link => {
    const [os, arch] = link.dataset.releaseAsset.split('/');
    link.href = base() + config.assets[os][arch];
  });
  options.forEach(option => option.setAttribute('aria-checked', String(option.dataset.channel === channel)));
}
applyChannel();

function setMenu(open, focusIndex) {
  menu.hidden = !open;
  toggle.setAttribute('aria-expanded', String(open));
  if (open) options[focusIndex ?? options.findIndex(option => option.dataset.channel === channel)].focus();
}
toggle.addEventListener('click', () => setMenu(menu.hidden));
toggle.addEventListener('keydown', event => {
  if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
  event.preventDefault();
  setMenu(true, event.key === 'ArrowUp' ? options.length - 1 : 0);
});
menu.addEventListener('keydown', event => {
  const index = options.indexOf(event.target);
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    options[(index + (event.key === 'ArrowDown' ? 1 : options.length - 1)) % options.length].focus();
  } else if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    choose(event.target);
  } else if (event.key === 'Escape') {
    event.preventDefault();
    setMenu(false);
    toggle.focus();
  }
});
options.forEach(option => option.addEventListener('click', () => choose(option)));
document.addEventListener('click', event => {
  if (!menu.hidden && !split.contains(event.target)) setMenu(false);
});

function choose(option) {
  channel = option.dataset.channel;
  applyChannel();
  setMenu(false);
  startDownload();
}

button.addEventListener('click', startDownload);

function startDownload() {
  if (platform === 'windows') {
    window.location.assign(base() + config.assets.windows.x86_64);
    return;
  }
  document.getElementById('linux-install').hidden = platform !== 'linux';
  document.getElementById('macos-install').hidden = platform !== 'macos';
  document.getElementById('desktop-install').hidden = platform === 'linux' || platform === 'macos';
  document.getElementById('install-title').textContent = platform === 'linux' ? 'Install on Linux' : platform === 'macos' ? 'Download for macOS' : 'Choose your desktop';
  document.getElementById('copy-command').textContent = 'Copy';
  document.getElementById('copy-status').textContent = '';
  dialog.showModal();
}
document.getElementById('close-dialog').addEventListener('click', () => dialog.close());
dialog.addEventListener('click', event => {
  if (event.target === dialog) {
    const rect = dialog.getBoundingClientRect();
    if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) dialog.close();
  }
});
document.getElementById('copy-command').addEventListener('click', async event => {
  const copyButton = event.currentTarget;
  try {
    await navigator.clipboard.writeText(installCommand());
    copyButton.textContent = 'Copied';
    document.getElementById('copy-status').textContent = 'Install command copied.';
  } catch {
    const input = document.getElementById('install-command');
    input.focus();
    input.select();
    document.getElementById('copy-status').textContent = 'Select and copy the command above.';
  }
});
