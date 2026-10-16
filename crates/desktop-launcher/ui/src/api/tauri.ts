// The real bridge to the Rust launcher.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Api } from './api';
import type { GameState, UpdateState } from './types';

/** Subscribes synchronously and detaches once the asynchronous listener registration lands. */
function subscribe<T>(event: string, cb: (payload: T) => void): () => void {
  let unlisten: UnlistenFn | undefined;
  let disposed = false;
  void listen<T>(event, (e) => cb(e.payload)).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  });
  return () => {
    disposed = true;
    unlisten?.();
  };
}

export const tauriApi: Api = {
  getInstallState: () => invoke('get_install_state'),
  getUpdateState: () => invoke('get_update_state'),
  checkForUpdates: () => invoke('check_for_updates'),
  installUpdate: () => invoke('install_update'),
  setChannel: (channel) => invoke('set_channel', { channel }),
  launchGame: (opts) => invoke('launch_game', { opts: opts ?? null }),
  getGameState: () => invoke('get_game_state'),
  getNews: () => invoke('get_news'),
  getFeaturedServers: () => invoke('get_featured_servers'),
  openExternal: (url) => invoke('open_external', { url }),
  openLogs: () => invoke('open_logs'),
  getRecentServers: () => invoke('get_recent_servers'),
  getChangelog: () => invoke('get_changelog'),
  onUpdateState: (cb) => subscribe<UpdateState>('update-state', cb),
  onGameState: (cb) => subscribe<GameState>('game-state', cb),
};
