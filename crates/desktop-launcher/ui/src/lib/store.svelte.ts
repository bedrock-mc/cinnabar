import { api, type Changelog, type RecentServer, type Channel, type FeaturedServer, type GameState, type InstallState, type News, type UpdateState } from '../api';
import { getAccount, signIn, type Account, type Server } from '../api/proposed';
import type { ActionIntent } from './play';
import type { Platform } from '../api';
import { platform as guessedPlatform } from './window';

export type Load<T> = { status: 'loading' } | { status: 'ready'; value: T } | { status: 'error' };
export type Page = 'home' | 'servers' | 'settings';

const PAGES: Page[] = ['home', 'servers', 'settings'];

function pageFromHash(): Page {
  const name = location.hash.replace(/^#\/?/, '');
  return PAGES.find((p) => p === name) ?? 'home';
}

/** Fetches into `set`; a refresh keeps showing the previous value until the new one lands. */
async function load<T>(fetch: () => Promise<T>, set: (v: Load<T>) => void, current: Load<T>) {
  if (current.status === 'error') set({ status: 'loading' });
  try {
    set({ status: 'ready', value: await fetch() });
  } catch {
    if (current.status !== 'ready') set({ status: 'error' });
  }
}

class LauncherStore {
  page = $state<Page>(pageFromHash());
  install = $state<Load<InstallState>>({ status: 'loading' });
  update = $state<UpdateState>();
  game = $state<GameState>();
  news = $state<Load<News>>({ status: 'loading' });
  servers = $state<Load<Server[]>>({ status: 'loading' });
  /** Host platform: the installer's report once loaded, the user agent before that. */
  get platform(): Platform {
    return this.install.status === 'ready' ? this.install.value.platform : guessedPlatform;
  }

  /** Undefined while loading or when the bridge has no account support yet. */
  account = $state<Account>();
  recent = $state<RecentServer[]>([]);
  changelog = $state<Changelog>();
  now = $state(Date.now());
  /** Set by "Update & play" so a finished install launches the game. */
  playAfterInstall = $state(false);
  /** Which request a failed update came from, so Retry repeats it. */
  private lastUpdateAction: 'check' | 'install' = 'check';

  start(): () => void {
    const offUpdate = api.onUpdateState((s) => this.applyUpdate(s));
    const offGame = api.onGameState((s) => (this.game = s));
    const tick = setInterval(() => (this.now = Date.now()), 30_000);
    const onHash = () => (this.page = pageFromHash());
    addEventListener('hashchange', onHash);

    this.reloadInstall();
    api.getUpdateState().then((s) => (this.update ??= s), () => (this.update ??= { kind: 'idle' }));
    api.getGameState().then((s) => (this.game ??= s), () => (this.game ??= { kind: 'stopped' }));
    this.reloadNews();
    this.reloadServers();
    getAccount().then((a) => (this.account = a), () => undefined);
    api.getRecentServers().then((r) => (this.recent = r), () => undefined);
    api.getChangelog().then((c) => (this.changelog = c ?? undefined), () => undefined);

    return () => {
      offUpdate();
      offGame();
      clearInterval(tick);
      removeEventListener('hashchange', onHash);
    };
  }

  private applyUpdate(next: UpdateState) {
    const finishedInstall = this.update?.kind === 'installing' && (next.kind === 'upToDate' || next.kind === 'idle');
    this.update = next;
    if (next.kind === 'failed') this.playAfterInstall = false;
    if (!finishedInstall) return;
    this.reloadInstall();
    if (this.playAfterInstall) {
      this.playAfterInstall = false;
      void api.launchGame();
    }
  }

  /** News falls back to its cache when the network is down; that is the launcher's offline signal. */
  get offline(): boolean {
    return this.news.status === 'ready' && this.news.value.stale;
  }

  navigate(page: Page) {
    location.hash = page === 'home' ? '' : `/${page}`;
    this.page = page;
  }

  reloadInstall = () => load(api.getInstallState, (v) => (this.install = v), this.install);
  reloadNews = () => load(api.getNews, (v) => (this.news = v), this.news);
  reloadServers = () => load(api.getFeaturedServers, (v) => (this.servers = v), this.servers);

  play = () => api.launchGame();

  updateAndPlay = () => {
    this.playAfterInstall = true;
    this.lastUpdateAction = 'install';
    return api.installUpdate();
  };

  /** Starts a manual install (a downgrade) without launching afterwards. */
  installManually = () => {
    this.playAfterInstall = false;
    this.lastUpdateAction = 'install';
    return api.installUpdate();
  };

  check = () => {
    this.lastUpdateAction = 'check';
    return api.checkForUpdates();
  };

  /** Repeats the step that failed: a failed check checks again, any later step reinstalls. */
  retry = () => {
    const failedCheck = this.update?.kind === 'failed' ? this.update.stage === 'check' : this.lastUpdateAction !== 'install';
    return failedCheck ? api.checkForUpdates() : api.installUpdate();
  };

  join = (server: FeaturedServer) => api.launchGame({ server: { address: server.address, port: server.port } });

  setChannel = async (channel: Channel) => {
    if (this.install.status === 'ready') this.install = { status: 'ready', value: { ...this.install.value, channel } };
    await api.setChannel(channel);
    await this.reloadInstall();
  };

  open = (url: string) => api.openExternal(url);

  openLogs = () => api.openLogs();

  /** Runs an action offered by a notice or the status line. */
  act = (intent: ActionIntent) => {
    switch (intent) {
      case 'retry':
        return this.retry();
      case 'install':
        return this.installManually();
      case 'logs':
        return this.openLogs();
      case 'check':
        return this.check();
      case 'notes':
        return this.update?.kind === 'available' && this.update.notesUrl ? this.open(this.update.notesUrl) : undefined;
    }
  };

  rejoin = (server: RecentServer) => api.launchGame({ server: { address: server.address, port: server.port } });

  signIn = () => signIn();
}

export const store = new LauncherStore();
