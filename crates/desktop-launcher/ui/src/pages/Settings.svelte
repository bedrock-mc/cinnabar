<script lang="ts">
  import ArrowUpRight from '@lucide/svelte/icons/arrow-up-right';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import InlineState from '../components/InlineState.svelte';
  import Segmented from '../components/Segmented.svelte';
  import { store } from '../lib/store.svelte';
  import { CHANNEL_LABEL, updateStatus } from '../lib/play';
  import { DISCORD_URL, LICENSE_NAME, PRODUCT_NAME, SOURCE_URL } from '../lib/product';
  import type { Channel } from '../api';

  const CHANNEL_INFO: Record<Channel, string> = {
    stable: 'Release builds. The steadiest choice.',
    beta: 'Tested builds. Recommended for most players.',
    nightly: 'Builds from the latest changes, plus any newer Beta. Expect rough edges.',
  };
  const CHANNEL_ORDER: Channel[] = ['stable', 'beta', 'nightly'];

  const install = $derived(store.install.status === 'ready' ? store.install.value : undefined);
  // Only channels with real releases are offered; Stable appears once it publishes.
  const channels = $derived(CHANNEL_ORDER.filter((c) => install?.publishedChannels.includes(c)).map((c) => ({ value: c, label: CHANNEL_LABEL[c] })));
  const busy = $derived(['checking', 'downloading', 'verifying', 'installing'].includes(store.update?.kind ?? ''));
  // Dev and unsigned builds report why updates are off instead of a check result.
  const status = $derived(store.update?.kind === 'unavailable' ? store.update.reason : updateStatus(store.update, store.now)?.text);
  const downgrade = $derived(store.update?.kind === 'available' && store.update.downgrade ? store.update : undefined);

  interface Notice {
    name: string;
    version: string;
    license: string;
    text: string;
  }

  // The build writes licenses.json from the modules it actually bundled.
  let licenses = $state<Notice[] | 'error'>();
  async function loadLicenses(e: Event) {
    if (!(e.currentTarget as HTMLDetailsElement).open || Array.isArray(licenses)) return;
    try {
      const res = await fetch('licenses.json');
      licenses = res.ok ? ((await res.json()) as Notice[]) : 'error';
    } catch {
      licenses = 'error';
    }
  }
</script>

<div class="page">
  <h1>Settings</h1>

  <section aria-labelledby="updates-title">
    <h2 id="updates-title">Updates</h2>
    <div class="group">
      {#if store.install.status === 'error'}
        <div class="row"><InlineState message="Cinnabar couldn’t read its installation." onretry={() => void store.reloadInstall()} /></div>
      {:else}
        <div class="row">
          <div class="label">
            <span class="name">Release channel</span>
            <span class="hint">{install ? CHANNEL_INFO[install.channel] : ' '}</span>
          </div>
          {#if install}
            <Segmented label="Release channel" options={channels} value={install.channel} disabled={busy} onchange={(c) => void store.setChannel(c)} />
          {:else}
            <span class="placeholder"></span>
          {/if}
        </div>
        {#if install && downgrade}
          <div class="row manual">
            <div class="label">
              <span class="name">Install {CHANNEL_LABEL[downgrade.channel]} {downgrade.version} by hand</span>
              <span class="hint">It is older than your {CHANNEL_LABEL[install.installedChannel]} {install.version}. The installer opens and Cinnabar closes.</span>
            </div>
            <button class="button" disabled={store.game?.kind !== 'stopped'} onclick={() => void store.installManually()}>Open installer</button>
          </div>
        {:else if install && install.channel !== install.installedChannel}
          <p class="pending">
            Your installed build is from {CHANNEL_LABEL[install.installedChannel]}. {CHANNEL_LABEL[install.channel]} installs with the next update.
          </p>
        {/if}
        <div class="row">
          <div class="label">
            <span class="name">Game version</span>
            <span class="hint">{install ? `${install.version}, build ${install.build}, from ${CHANNEL_LABEL[install.installedChannel]}` : ' '}</span>
          </div>
          <button class="button" disabled={busy || store.game?.kind !== 'stopped' || store.update?.kind === 'unavailable'} onclick={() => void store.check()}>
            {#if store.update?.kind === 'checking'}<LoaderCircle class="spin" size={14} strokeWidth={2} aria-hidden="true" />{/if}
            Check for updates
          </button>
        </div>
        {#if status}<p class="status" aria-live="polite">{status}</p>{/if}
      {/if}
    </div>
  </section>

  <section aria-labelledby="about-title">
    <h2 id="about-title">About</h2>
    <div class="group">
      <div class="row">
        <div class="label">
          <span class="name">Launcher</span>
          <span class="hint">Version {install?.launcherVersion ?? '…'}</span>
        </div>
      </div>
      <div class="row">
        <p class="about">{PRODUCT_NAME} is an independent, source-available client for Bedrock servers. It is not affiliated with or endorsed by the makers of the original game.</p>
      </div>
      <button class="row link" onclick={() => void store.openLogs()}>
        <span class="label"><span class="name">Logs</span><span class="hint">The launcher’s and the game’s logs, for bug reports</span></span><ArrowUpRight size={16} strokeWidth={2} aria-hidden="true" />
      </button>
      <button class="row link" onclick={() => void store.open(SOURCE_URL)}>
        <span class="name">Source code</span><ArrowUpRight size={16} strokeWidth={2} aria-hidden="true" />
      </button>
      <button class="row link" onclick={() => void store.open(DISCORD_URL)}>
        <span class="name">Discord</span><ArrowUpRight size={16} strokeWidth={2} aria-hidden="true" />
      </button>
      <details class="row licenses" ontoggle={loadLicenses}>
        <summary>
          <span class="label">
            <span class="name">Licenses</span>
            <span class="hint">{PRODUCT_NAME} is licensed under {LICENSE_NAME}. Third-party notices for this launcher follow.</span>
          </span>
          <ChevronRight class="chev" size={16} strokeWidth={2} aria-hidden="true" />
        </summary>
        {#if licenses === 'error'}
          <InlineState message="The license list didn’t load." />
        {:else if licenses && licenses.length === 0}
          <InlineState message="Development builds don’t list licenses. Run a production build to see them." />
        {:else if licenses}
          <ul>
            {#each licenses as entry (entry.name)}
              <li>
                <details>
                  <summary><span class="pkg">{entry.name}</span> <span class="hint">{entry.version}, {entry.license}</span></summary>
                  <pre>{entry.text || 'This package ships no license file; its declared license is listed above.'}</pre>
                </details>
              </li>
            {/each}
          </ul>
        {/if}
      </details>
    </div>
  </section>
</div>

<style>
  .page {
    max-width: 720px;
    padding: 20px 40px 48px;
  }

  h1 {
    font: var(--t-page);
    letter-spacing: var(--track-display);
  }

  section {
    margin-top: 36px;
  }

  h2 {
    margin: 0 0 10px 4px;
    font: var(--t-label);
    color: var(--ink-2);
  }

  .group {
    border-radius: var(--r-lg);
    background: var(--glass-strong);
    -webkit-backdrop-filter: blur(24px) saturate(1.4);
    backdrop-filter: blur(24px) saturate(1.4);
    box-shadow: inset 0 0 0 1px var(--glass-edge);
    overflow: hidden;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    width: 100%;
    min-height: 64px;
    padding: 12px 20px;
    text-align: left;
  }

  .row + .row,
  .pending + .row,
  .row + .pending {
    box-shadow: inset 0 1px 0 var(--line);
  }

  .label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .name {
    font: var(--t-label);
    font-size: 14px;
  }

  .hint {
    font: var(--t-meta);
    font-size: 13px;
    color: var(--ink-3);
  }

  .manual .name {
    color: var(--warn);
  }

  .pending {
    padding: 10px 20px 12px;
    font: var(--t-meta);
    font-size: 13px;
    color: var(--warn);
  }

  .status {
    padding: 0 20px 14px;
    margin-top: -6px;
    font: var(--t-meta);
    color: var(--ink-3);
    text-align: right;
  }

  .placeholder {
    width: 180px;
    height: 38px;
    border-radius: 999px;
    background: var(--raise);
  }

  .button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex: none;
    height: 36px;
    padding: 0 16px;
    border-radius: 999px;
    background: var(--raise-2);
    font: var(--t-label);
    transition: background-color var(--t-fast) linear;
  }

  .button:hover:not(:disabled) {
    background: rgb(255 255 255 / 0.16);
  }

  .button:disabled {
    color: var(--ink-3);
  }

  .about {
    max-width: 60ch;
    font-size: 14px;
    color: var(--ink-2);
  }

  .link {
    color: var(--ink);
    transition: background-color var(--t-fast) linear;
  }

  .link:hover {
    background: var(--raise);
  }

  .link :global(svg) {
    color: var(--ink-3);
  }

  .licenses {
    display: block;
  }

  .licenses > summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    cursor: pointer;
    list-style: none;
  }

  .licenses > summary::-webkit-details-marker {
    display: none;
  }

  .licenses > summary :global(.chev) {
    flex: none;
    color: var(--ink-3);
    transition: transform var(--t-med) var(--ease-out);
  }

  .licenses[open] > summary :global(.chev) {
    transform: rotate(90deg);
  }

  .licenses ul {
    margin: 12px 0 0;
    padding: 0;
    list-style: none;
  }

  .licenses li {
    padding: 8px 0;
    box-shadow: inset 0 1px 0 var(--line);
  }

  .licenses li summary {
    cursor: pointer;
    font: var(--t-label);
  }

  .pkg {
    color: var(--ink);
  }

  pre {
    max-height: 220px;
    margin: 8px 0 0;
    padding: 12px;
    overflow: auto;
    border-radius: var(--r-sm);
    background: rgb(0 0 0 / 0.3);
    color: var(--ink-2);
    font-size: 11px;
    white-space: pre-wrap;
    user-select: text;
    -webkit-user-select: text;
  }
</style>
