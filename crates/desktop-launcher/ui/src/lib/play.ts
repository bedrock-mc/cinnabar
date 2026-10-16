import type { Channel, GameState, InstallState, UpdateState } from '../api';
import { formatAgo, formatBytes } from './format';

export type PrimaryIntent = 'play' | 'update' | 'retry' | 'none';

export interface Primary {
  label: string;
  detail?: string;
  intent: PrimaryIntent;
  /** 0..1 fill while downloading; null for an indeterminate busy state. */
  progress?: number | null;
  tone: 'ore' | 'busy' | 'running' | 'disabled';
}

export type ActionIntent = 'retry' | 'install' | 'logs' | 'check' | 'notes';

export interface Notice {
  tone: 'info' | 'warn' | 'error';
  title: string;
  body: string;
  action?: { label: string; intent: ActionIntent };
}

/** One quiet line under Play; problems that don't block playing never get more than this. */
export interface Status {
  tone: 'muted' | 'warn';
  text: string;
  busy?: boolean;
  action?: { label: string; intent: ActionIntent };
}

export type Stage = 'download' | 'verify' | 'install';

export interface PlayModel {
  primary: Primary;
  secondary?: { label: string; intent: 'playInstalled' };
  /** Reserved for states that block play or need a deliberate decision. */
  notice?: Notice;
  status?: Status;
  stage?: Stage;
}

export const CHANNEL_LABEL: Record<Channel, string> = { stable: 'Stable', beta: 'Beta', nightly: 'Nightly' };

/** Whether installing hands off to the platform installer (and closes) rather than relaunching. */
export function handsOff(install: InstallState | undefined): boolean {
  return install?.platform === 'windows';
}

/** Unpublished builds report `dev` and never update themselves. */
export function isDevBuild(install: InstallState | undefined): boolean {
  return install?.build === 'dev';
}

export function updateStatus(update: UpdateState | undefined, now: number): Status | undefined {
  switch (update?.kind) {
    case 'checking':
      return { tone: 'muted', text: 'Checking for updates', busy: true };
    case 'upToDate':
      return { tone: 'muted', text: `Up to date, checked ${formatAgo(update.lastChecked, now)}`, action: { label: 'Check now', intent: 'check' } };
    case 'idle':
      return { tone: 'muted', text: update.lastChecked ? `Checked ${formatAgo(update.lastChecked, now)}` : 'Not checked yet', action: { label: 'Check now', intent: 'check' } };
    case 'failed':
      return {
        tone: 'warn',
        text: update.stage === 'check' ? 'Couldn’t check for updates' : 'Update didn’t finish; your build is unchanged',
        action: update.retryable ? { label: 'Try again', intent: 'retry' } : undefined,
      };
    default:
      return undefined;
  }
}

/**
 * Derives what the launchpad offers from the three live states. Game state wins over update
 * state because an update cannot install while the game runs.
 */
export function playModel(install: InstallState | undefined, installFailed: boolean, update: UpdateState | undefined, game: GameState | undefined, now: number): PlayModel {
  if (installFailed) {
    return {
      primary: { label: 'Play', intent: 'none', tone: 'disabled' },
      notice: { tone: 'error', title: 'Cinnabar couldn’t read its installation', body: 'Reinstall Cinnabar from the website. Your worlds and settings are kept.' },
    };
  }
  if (!install || !game) return { primary: { label: 'Play', intent: 'none', tone: 'disabled' } };

  if (game.kind === 'starting') return { primary: { label: 'Starting', intent: 'none', tone: 'busy', progress: null } };
  if (game.kind === 'running') {
    return { primary: { label: 'In game', detail: `Since ${formatAgo(game.startedAt, now)}`, intent: 'none', tone: 'running' } };
  }

  const play: Primary = { label: 'Play', intent: 'play', tone: 'ore' };
  const crashed = game.lastExit?.crashed
    ? ({ tone: 'warn', text: 'Cinnabar closed unexpectedly last time', action: { label: 'View log', intent: 'logs' } } as const satisfies Status)
    : undefined;

  switch (update?.kind) {
    case 'downloading': {
      const total = update.totalBytes;
      const bytes = total ? `${formatBytes(update.receivedBytes)} of ${formatBytes(total)}` : `${formatBytes(update.receivedBytes)} so far`;
      return {
        primary: { label: 'Downloading', detail: `${update.version}, ${bytes}`, intent: 'none', tone: 'busy', progress: total ? update.receivedBytes / total : null },
        stage: 'download',
      };
    }
    case 'verifying':
      return { primary: { label: 'Verifying', detail: `Checking ${update.version}’s signature`, intent: 'none', tone: 'busy', progress: null }, stage: 'verify' };
    case 'installing':
      // Windows hands the verified package to its installer and closes; elsewhere a helper swaps
      // the files and relaunches straight into the game.
      return handsOff(install)
        ? { primary: { label: 'Opening installer', detail: `Cinnabar closes while ${update.version} installs`, intent: 'none', tone: 'busy', progress: null }, stage: 'install' }
        : { primary: { label: 'Installing', detail: `Cinnabar restarts into ${update.version}`, intent: 'none', tone: 'busy', progress: null }, stage: 'install' };
    case 'available': {
      if (update.downgrade) {
        // Going back to an older channel release is a manual install, never the default action.
        return {
          primary: play,
          notice: {
            tone: 'info',
            title: `${CHANNEL_LABEL[update.channel]} is behind your build`,
            body: `You have ${CHANNEL_LABEL[install.installedChannel]} ${install.version}. Moving to ${CHANNEL_LABEL[update.channel]} ${update.version} opens the installer and closes Cinnabar.`,
            action: { label: `Install ${update.version}`, intent: 'install' },
          },
        };
      }
      const size = update.sizeBytes ? `, ${formatBytes(update.sizeBytes)}` : '';
      const primary: Primary = { label: 'Update & play', detail: `${update.version}${size}`, intent: 'update', tone: 'ore' };
      const notes = update.notesUrl ? { label: `What’s new in ${update.version}`, intent: 'notes' as const } : undefined;
      if (update.required) {
        return { primary, status: { tone: 'warn', text: 'A server needs a newer build', action: notes } };
      }
      return { primary, secondary: { label: `Play ${install.version}`, intent: 'playInstalled' }, status: crashed ?? (notes ? { tone: 'muted', text: '', action: notes } : undefined) };
    }
    case 'failed':
      if (!update.canPlayInstalled) {
        return {
          primary: update.retryable ? { label: 'Try again', intent: 'retry', tone: 'ore' } : { label: 'Play', intent: 'none', tone: 'disabled' },
          notice: { tone: 'error', title: 'This build can’t start', body: update.message },
        };
      }
      return { primary: play, status: crashed ?? updateStatus(update, now) };
    case 'unavailable':
      // Dev and unsigned builds don't update; the identity line carries a quiet tag instead.
      return { primary: play, status: crashed };
    default:
      return { primary: play, status: crashed ?? updateStatus(update, now) };
  }
}

/** Why a server's Play button is unavailable, or undefined when joining can start now. */
export function joinBlockedReason(update: UpdateState | undefined, game: GameState | undefined): string | undefined {
  if (game && game.kind !== 'stopped') return 'Cinnabar is already running';
  switch (update?.kind) {
    case 'downloading':
    case 'verifying':
    case 'installing':
      return 'Wait for the update to finish';
    case 'available':
      return update.required && !update.downgrade ? 'Update Cinnabar to join' : undefined;
    case 'failed':
      return update.canPlayInstalled ? undefined : 'Cinnabar can’t start until the update is fixed';
    default:
      return undefined;
  }
}

/** How much of the diorama's vein glows: the download's progress, dark when the build can't start. */
export function veinFill(model: PlayModel): number {
  if (model.stage === 'download') return model.primary.progress ?? 0;
  if (model.notice?.tone === 'error') return 0;
  return 1;
}
