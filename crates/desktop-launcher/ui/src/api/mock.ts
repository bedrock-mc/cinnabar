// Synthetic data for design work and screenshots. Server names, addresses and news are invented.
// Pick a state with `?scenario=<name>`, a host with `?platform=`; `&freeze` stops simulated progress.
import { mockSkin } from '../lib/diorama/textures';
import type { Account, Server } from './proposed';

/** A 16:9 voxel-skyline poster drawn as SVG, so mock news art never ships as an image file. */
function poster(top: string, bottom: string, ground: string, stars: boolean): string {
  const blocks = [3, 5, 4, 7, 6, 8, 5, 4, 6, 9, 7, 5, 3, 4, 6, 5]
    .map((h, i) => `<rect x="${i * 40}" y="${360 - h * 18}" width="40" height="${h * 18}" fill="${ground}"/>`)
    .join('');
  const sky = stars
    ? Array.from({ length: 40 }, (_, i) => `<rect x="${(i * 97) % 640}" y="${(i * 53) % 180}" width="3" height="3" fill="#fff" opacity="0.7"/>`).join('')
    : '<circle cx="470" cy="150" r="46" fill="#ffd9a0" opacity="0.9"/>';
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${top}"/><stop offset="1" stop-color="${bottom}"/></linearGradient></defs><rect width="640" height="360" fill="url(#s)"/>${sky}${blocks}</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

const nightArt = poster('#0b1030', '#3a2458', '#140d24', true);
const dawnArt = poster('#f08a5d', '#f6c48c', '#5a2a2a', false);
import type { Api, Changelog, RecentServer, Channel, GameState, InstallState, News, Platform, UpdateState } from './types';

const params = new URLSearchParams(typeof location === 'undefined' ? '' : location.search);
const scenario = params.get('scenario') ?? 'idle';
const frozen = params.has('freeze');

const MINUTE = 60_000;
const ago = (ms: number) => new Date(Date.now() - ms).toISOString();
const NEXT_VERSION = '0.1.22';
const UPDATE_BYTES = 118_437_120;

function hostPlatform(): Platform {
  const forced = params.get('platform');
  if (forced === 'macos' || forced === 'windows' || forced === 'linux') return forced;
  const ua = typeof navigator === 'undefined' ? '' : navigator.userAgent;
  return /Mac/.test(ua) ? 'macos' : /Windows/.test(ua) ? 'windows' : 'linux';
}

// The downgrade scenario has a newer Nightly build installed while Beta is selected.
const downgrading = scenario === 'downgrade';
const install: InstallState = {
  version: downgrading ? '0.1.23' : '0.1.21',
  build: downgrading ? 'n412' : scenario === 'dev' || scenario === 'bare' ? 'dev' : '5ef4062',
  channel: scenario === 'nightly' ? 'nightly' : 'beta',
  installedChannel: downgrading ? 'nightly' : 'beta',
  publishedChannels: ['beta', 'nightly'],
  launcherVersion: '0.1.21',
  platform: hostPlatform(),
  nativeApiVersion: 1,
};

const available = (required: boolean): UpdateState => ({
  kind: 'available',
  version: NEXT_VERSION,
  channel: install.channel,
  notesUrl: 'https://example.com/cinnabar/notes/0.1.22',
  sizeBytes: UPDATE_BYTES,
  required,
  downgrade: false,
});

const initialUpdate: Record<string, UpdateState> = {
  never: { kind: 'idle' },
  checking: { kind: 'checking' },
  available: available(false),
  required: available(true),
  downloading: { kind: 'downloading', version: NEXT_VERSION, receivedBytes: 50_593_792, totalBytes: UPDATE_BYTES },
  verifying: { kind: 'verifying', version: NEXT_VERSION },
  installing: { kind: 'installing', version: NEXT_VERSION },
  handoff: { kind: 'installing', version: NEXT_VERSION },
  downgrade: { kind: 'available', version: '0.1.21', channel: 'beta', sizeBytes: 112_400_000, required: false, downgrade: true },
  failed: {
    kind: 'failed',
    message: 'The download was interrupted (connection reset). Your installed version is unchanged.',
    stage: 'download',
    retryable: true,
    canPlayInstalled: true,
  },
  dev: { kind: 'unavailable', reason: 'This build is unsigned, so it doesn’t update itself.' },
  bare: { kind: 'unavailable', reason: 'This build is unsigned, so it doesn’t update itself.' },
  'failed-blocking': {
    kind: 'failed',
    message: 'The installed files failed verification, so this version cannot start.',
    stage: 'verify',
    retryable: true,
    canPlayInstalled: false,
  },
  offline: {
    kind: 'failed',
    message: 'Couldn’t reach the update server. Check your connection.',
    stage: 'check',
    retryable: true,
    canPlayInstalled: true,
  },
};

const initialGame: Record<string, GameState> = {
  starting: { kind: 'starting' },
  running: { kind: 'running', startedAt: ago(23 * MINUTE) },
  crashed: { kind: 'stopped', lastExit: { code: 134, crashed: true } },
};

let update: UpdateState = initialUpdate[scenario] ?? { kind: 'upToDate', lastChecked: ago(3 * MINUTE) };
let game: GameState = initialGame[scenario] ?? { kind: 'stopped' };

const updateListeners = new Set<(s: UpdateState) => void>();
const gameListeners = new Set<(s: GameState) => void>();

function setUpdate(next: UpdateState) {
  update = next;
  for (const cb of updateListeners) cb(next);
}

function setGame(next: GameState) {
  game = next;
  for (const cb of gameListeners) cb(next);
}

const wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
const never = <T>() => new Promise<T>(() => {});

/** Resolves like a local IPC call, never resolves under `loading`, and rejects under `errors`. */
async function respond<T>(value: () => T, failIn: string[] = []): Promise<T> {
  if (scenario === 'loading') return never();
  await wait(180);
  if (failIn.includes(scenario)) throw new Error('mock failure');
  return value();
}

const news: News = {
  stale: false,
  fetchedAt: ago(2 * MINUTE),
  items: [
    {
      id: 'n1',
      title: 'Cinnabar 0.1.21: optional frame limits and lower input latency on Metal',
      summary: 'Cap your frame rate from Video settings. Apple GPUs now queue fewer frames, so aiming feels tighter.',
      url: 'https://example.com/cinnabar/news/0-1-21',
      publishedAt: ago(26 * 60 * MINUTE),
      tag: 'Release',
      imageUrl: nightArt,
    },
    {
      id: 'n2',
      title: 'Beta or Nightly: which channel should you play on?',
      summary: 'Nightly gets every change as it lands. Beta waits until a build has been played for a while.',
      url: 'https://example.com/cinnabar/news/channels',
      publishedAt: ago(4 * 24 * 60 * MINUTE),
      tag: 'Guide',
    },
    {
      id: 'n3',
      title: 'Server owners: test your resource packs before the weekend',
      summary: 'Pack loading changed in 0.1.20. Join with a Nightly build and tell us what looks wrong.',
      url: 'https://example.com/cinnabar/news/packs',
      imageUrl: dawnArt,
      publishedAt: ago(9 * 24 * 60 * MINUTE),
      tag: 'Servers',
    },
    {
      id: 'n4',
      title: 'Bucket and liquid picking fixes',
      summary: 'Empty buckets now pick the source block you are looking at instead of the one behind it.',
      url: 'https://example.com/cinnabar/news/buckets',
      publishedAt: ago(12 * 24 * 60 * MINUTE),
    },
  ],
};

const servers: Server[] = [
  { id: 's1', name: 'Emberfall', address: 'play.emberfall.example', port: 19132, description: 'Seasonal survival with a player-run market and town claims.', online: 2481, motd: 'Season 7 opens Friday. Claim your plot early.', tags: ['Survival', 'Economy'], pingMs: 38 },
  { id: 's2', name: 'Northwind Duels', address: 'pvp.northwind.example', port: 19132, description: 'Practice arenas, ranked duels and weekly brackets.', online: 1960, motd: 'Ranked queue is live. Weekly bracket at 8 pm.', tags: ['PvP', 'Ranked'], pingMs: 52 },
  { id: 's3', name: 'Tidewater Isles', address: 'tidewater.example', port: 19133, description: 'Skyblock across floating archipelagos, solo or in crews of four.', online: 1302, motd: 'New island biome: the coral shelf.', tags: ['Skyblock'], pingMs: 71 },
  { id: 's4', name: 'Quarry Kings', address: 'mc.quarrykings.example', port: 19132, description: 'Eight-player mining races. First crew to the core wins.', online: 864, motd: 'Double ore weekend.', tags: ['Minigames', 'Teams'], pingMs: 44 },
  { id: 's5', name: 'Hollowmere', address: 'hollowmere.example', port: 19132, description: 'Story-driven adventure maps with a new chapter every Friday.', online: 412, tags: ['Adventure'], pingMs: 118 },
  { id: 's6', name: 'Brickyard Creative', address: 'build.brickyard.example', port: 19132, description: 'Plots, build battles and a monthly showcase.', online: 233, tags: ['Creative'], pingMs: 63 },
  { id: 's7', name: 'Lanternfall', address: 'lanternfall.example', port: 19140 },
  { id: 's9', name: 'Skyward Trials', address: '8c0f3e52-6d1b-4f7a-9a21-3b5e9d7c4a10', port: 0, description: 'A parkour experience with weekly leaderboards.', online: 318, tags: ['Experience', 'Parkour'] },
  { id: 's8', name: 'Glasshouse SMP', address: 'glasshouse.example', port: 19132, description: 'Small whitelist-free survival. Be kind, build slow.', online: 57, tags: ['Survival', 'Small'], pingMs: 196 },
];

async function simulateInstall() {
  const version = NEXT_VERSION;
  const steps = 48;
  for (let i = 0; i <= steps; i++) {
    if (scenario === 'failed' && i === 20) {
      setUpdate(initialUpdate.failed!);
      return;
    }
    setUpdate({ kind: 'downloading', version, receivedBytes: Math.round((UPDATE_BYTES * i) / steps), totalBytes: UPDATE_BYTES });
    await wait(110);
  }
  setUpdate({ kind: 'verifying', version });
  await wait(1100);
  setUpdate({ kind: 'installing', version });
  // On Windows the real launcher hands off to the installer and quits here.
  if (install.platform === 'windows') return;
  await wait(1400);
  install.version = version;
  install.build = '9c41d07';
  install.installedChannel = install.channel;
  setUpdate({ kind: 'upToDate', lastChecked: new Date().toISOString() });
}

/** Recent joins for the quick-rejoin row; empty on first run. */
export async function mockRecentServers(): Promise<RecentServer[]> {
  if (scenario === 'loading') return never();
  await wait(140);
  if (scenario === 'empty' || scenario === 'bare' || params.has('first-run')) return [];
  return [
    { name: 'Emberfall', address: 'play.emberfall.example', port: 19132, lastPlayedAt: ago(2 * 60 * MINUTE) },
    { name: 'Northwind Duels', address: 'pvp.northwind.example', port: 19132, lastPlayedAt: ago(26 * 60 * MINUTE) },
    { name: 'Skyward Trials', address: '8c0f3e52-6d1b-4f7a-9a21-3b5e9d7c4a10', port: 0, lastPlayedAt: ago(4 * 24 * 60 * MINUTE) },
  ];
}

/** Notes for the installed build; `?new` marks it as the first launch of this version. */
export async function mockChangelog(): Promise<Changelog | null> {
  await wait(100);
  if (scenario === 'bare') return null;
  return {
    version: install.version,
    publishedAt: ago(26 * 60 * MINUTE),
    highlights: [
      'Optional frame limits in Video settings',
      'Lower input latency on Apple GPUs',
      'Empty buckets pick the block you aim at',
    ],
    url: 'https://example.com/cinnabar/notes/0.1.21',
    unseen: params.has('new'),
  };
}

/** Signed in by default; `?account=out` signs out, and the offline scenario keeps a cached identity. */
export async function mockAccount(): Promise<Account | undefined> {
  if (scenario === 'loading') return never();
  await wait(120);
  const forced = params.get('account');
  if (forced === 'out') return { kind: 'signedOut' };
  if (scenario === 'bare') return undefined;
  const skinUrl = mockSkin().toDataURL();
  return scenario === 'offline' ? { kind: 'offline', gamertag: 'Vermillion Fox', skinUrl } : { kind: 'signedIn', gamertag: 'Vermillion Fox', skinUrl };
}

export const mockApi: Api = {
  getInstallState: () => respond(() => ({ ...install, publishedChannels: [...install.publishedChannels] }), ['errors']),
  getUpdateState: () => respond(() => update),
  async checkForUpdates() {
    setUpdate({ kind: 'checking' });
    if (frozen) return;
    await wait(1200);
    if (scenario === 'offline') setUpdate(initialUpdate.offline!);
    else if (scenario === 'idle' || scenario === 'never') setUpdate({ kind: 'upToDate', lastChecked: new Date().toISOString() });
    else setUpdate(available(scenario === 'required'));
  },
  async installUpdate() {
    if (!frozen) void simulateInstall();
  },
  async setChannel(channel: Channel) {
    install.channel = channel;
    await mockApi.checkForUpdates();
  },
  async launchGame() {
    setGame({ kind: 'starting' });
    if (frozen) return;
    await wait(1600);
    setGame({ kind: 'running', startedAt: new Date().toISOString() });
  },
  getGameState: () => respond(() => game),
  getNews: () =>
    respond(() => {
      if (scenario === 'offline') return { ...news, stale: true, fetchedAt: ago(5 * 60 * MINUTE) };
      if (scenario === 'empty' || scenario === 'bare') return { items: [], stale: scenario === 'bare', fetchedAt: ago(MINUTE) };
      return news;
    }, ['errors']),
  getFeaturedServers: () => respond(() => (scenario === 'empty' || scenario === 'bare' ? [] : servers), ['errors', 'offline']),
  async openExternal(url: string) {
    window.open(url, '_blank', 'noopener');
  },
  async openLogs() {},
  getRecentServers: () => mockRecentServers(),
  getChangelog: () => mockChangelog(),
  onUpdateState(cb) {
    updateListeners.add(cb);
    return () => updateListeners.delete(cb);
  },
  onGameState(cb) {
    gameListeners.add(cb);
    return () => gameListeners.delete(cb);
  },
};
