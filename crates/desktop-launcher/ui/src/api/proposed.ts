// Data the UI can show but the bridge contract does not carry yet. In Tauri these resolve to
// nothing until the builder adds them, and the UI leaves out whatever is missing.
import { inTauri } from './index';
import type { FeaturedServer } from './types';

export type Account =
  | { kind: 'signedIn'; gamertag: string; skinUrl?: string }
  | { kind: 'signedOut' }
  /** Cached identity while sign-in services are unreachable; the installed game still plays. */
  | { kind: 'offline'; gamertag: string; skinUrl?: string };

export type Server = FeaturedServer;

async function fromMock<T>(pick: (mock: typeof import('./mock')) => Promise<T>): Promise<T | undefined> {
  if (inTauri) return undefined;
  return pick(await import('./mock'));
}

export const getAccount = () => fromMock((m) => m.mockAccount());

export async function signIn(): Promise<void> {}
