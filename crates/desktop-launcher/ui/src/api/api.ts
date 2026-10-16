// The launcher bridge. Command names are the snake_case forms Rust registers; a Rust test
// checks that `tauri.ts` invokes exactly those commands and events.
import type {
  Changelog,
  Channel,
  FeaturedServer,
  GameState,
  InstallState,
  LaunchOptions,
  News,
  RecentServer,
  UpdateState,
} from './types';

export interface Api {
  getInstallState(): Promise<InstallState>;
  getUpdateState(): Promise<UpdateState>;
  checkForUpdates(): Promise<void>;
  /** Downloads and installs the offered release; call only from an explicit click. */
  installUpdate(): Promise<void>;
  setChannel(channel: Channel): Promise<void>;
  launchGame(opts?: LaunchOptions): Promise<void>;
  getGameState(): Promise<GameState>;
  getNews(): Promise<News>;
  getFeaturedServers(): Promise<FeaturedServer[]>;
  openExternal(url: string): Promise<void>;
  /** Opens the folder holding the launcher's and the game's logs. */
  openLogs(): Promise<void>;
  /** Saved servers the game joined, newest first. */
  getRecentServers(): Promise<RecentServer[]>;
  /** The installed build's highlights; `unseen` only on this version's first read. */
  getChangelog(): Promise<Changelog | null>;
  /** Event `update-state`; returns the unsubscribe function. */
  onUpdateState(cb: (state: UpdateState) => void): () => void;
  /** Event `game-state`; returns the unsubscribe function. */
  onGameState(cb: (state: GameState) => void): () => void;
}
