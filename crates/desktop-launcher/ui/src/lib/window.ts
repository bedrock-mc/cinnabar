import { inTauri } from '../api';

export type Platform = 'macos' | 'windows' | 'linux';

/** The host platform; `?platform=` overrides it in the browser preview. */
export const platform: Platform = (() => {
  const forced = new URLSearchParams(location.search).get('platform');
  if (forced === 'macos' || forced === 'windows' || forced === 'linux') return forced;
  const ua = navigator.userAgent;
  if (/Mac/.test(ua)) return 'macos';
  if (/Windows/.test(ua)) return 'windows';
  return 'linux';
})();

type WindowAction = 'minimize' | 'toggleMaximize' | 'close';

/** Runs a window action in Tauri; a no-op in the browser preview. */
export async function windowAction(action: WindowAction): Promise<void> {
  if (!inTauri) return;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  await getCurrentWindow()[action]();
}
