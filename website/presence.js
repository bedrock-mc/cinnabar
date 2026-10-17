(() => {
  const label = document.getElementById('online-count');
  let timer;
  let pending = false;

  /** Refreshes the aggregate while visible and clears unavailable or invalid counts. */
  async function refresh() {
    if (document.hidden || pending) return;
    pending = true;
    window.clearTimeout(timer);
    try {
      const response = await fetch(window.CINNABAR_DOWNLOADS.online_count_url, {
        credentials: 'omit', referrerPolicy: 'no-referrer', signal: AbortSignal.timeout(5000),
      });
      if (!response.ok) throw new Error('Count unavailable');
      const body = await response.json();
      if (!Number.isSafeInteger(body?.online) || body.online < 0) throw new Error('Invalid count');
      label.textContent = document.hidden ? '' : body.online === 1
        ? '1 person is using Cinnabar right now'
        : `${body.online.toLocaleString()} people are using Cinnabar right now`;
    } catch {
      label.textContent = '';
    } finally {
      pending = false;
      if (!document.hidden) timer = window.setTimeout(refresh, 60_000);
    }
  }

  document.addEventListener('visibilitychange', () => {
    window.clearTimeout(timer);
    label.textContent = '';
    if (!document.hidden) void refresh();
  });
  void refresh();
})();
