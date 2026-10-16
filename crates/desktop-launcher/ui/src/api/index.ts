import type { Api } from './types';

export type * from './types';

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

/** True inside the Tauri webview; false in a plain browser, where the mock serves design work. */
export const inTauri = typeof window !== 'undefined' && window.__TAURI_INTERNALS__ !== undefined;

export const api: Api = inTauri ? (await import('./tauri')).tauriApi : (await import('./mock')).mockApi;
