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
document.getElementById('install-command').value = config.install_command;
document.querySelectorAll('[data-release-asset]').forEach(link => {
  const [os, arch] = link.dataset.releaseAsset.split('/');
  link.href = config.download_base + config.assets[os][arch];
});

button.addEventListener('click', () => {
  if (platform === 'windows') {
    window.location.assign(config.download_base + config.assets.windows.x86_64);
    return;
  }
  document.getElementById('linux-install').hidden = platform !== 'linux';
  document.getElementById('macos-install').hidden = platform !== 'macos';
  document.getElementById('desktop-install').hidden = platform === 'linux' || platform === 'macos';
  document.getElementById('install-title').textContent = platform === 'linux' ? 'Install on Linux' : platform === 'macos' ? 'Download for macOS' : 'Choose your desktop';
  document.getElementById('copy-command').textContent = 'Copy';
  document.getElementById('copy-status').textContent = '';
  dialog.showModal();
});
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
    await navigator.clipboard.writeText(config.install_command);
    copyButton.textContent = 'Copied';
    document.getElementById('copy-status').textContent = 'Install command copied.';
  } catch {
    const input = document.getElementById('install-command');
    input.focus();
    input.select();
    document.getElementById('copy-status').textContent = 'Select and copy the command above.';
  }
});
