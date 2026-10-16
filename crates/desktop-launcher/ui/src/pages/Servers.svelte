<script lang="ts">
  import Search from '@lucide/svelte/icons/search';
  import X from '@lucide/svelte/icons/x';
  import Copy from '@lucide/svelte/icons/copy';
  import Check from '@lucide/svelte/icons/check';
  import ServerArt from '../components/ServerArt.svelte';
  import ServerCard from '../components/ServerCard.svelte';
  import ServerPlay from '../components/ServerPlay.svelte';
  import OnlineCount from '../components/OnlineCount.svelte';
  import Ping from '../components/Ping.svelte';
  import InlineState from '../components/InlineState.svelte';
  import { store } from '../lib/store.svelte';
  import { formatAddress, isExperience } from '../lib/format';
  import type { Server } from '../api/proposed';

  let query = $state(new URLSearchParams(location.search).get('q') ?? '');
  let tag = $state<string>();
  let copied = $state<string>();

  const all = $derived<Server[]>(store.servers.status === 'ready' ? store.servers.value : []);
  // Filters come from the listings themselves, most common first, so they never go stale.
  const tags = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const s of all) for (const t of s.tags ?? []) counts.set(t, (counts.get(t) ?? 0) + 1);
    return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, 6).map(([t]) => t);
  });
  const results = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return all.filter(
      (s) =>
        (!tag || s.tags?.includes(tag)) &&
        (!q || [s.name, s.description ?? '', s.motd ?? '', s.address, ...(s.tags ?? [])].some((f) => f.toLowerCase().includes(q))),
    );
  });
  const filtering = $derived(query.trim() !== '' || tag !== undefined);
  const spotlight = $derived(filtering ? undefined : results[0]);
  const rest = $derived(spotlight ? results.slice(1) : results);

  async function copyAddress(server: Server) {
    try {
      await navigator.clipboard.writeText(formatAddress(server.address, server.port));
      copied = server.id;
      setTimeout(() => (copied = copied === server.id ? undefined : copied), 1600);
    } catch {
      copied = undefined;
    }
  }
</script>

<div class="page">
  <header class="head">
    <div>
      <h1>Servers</h1>
      <p class="lede">Play starts Cinnabar and joins the server directly.</p>
    </div>
    <label class="search">
      <Search size={16} strokeWidth={2} aria-hidden="true" />
      <span class="visually-hidden">Search servers</span>
      <input type="search" placeholder="Search servers" bind:value={query} spellcheck="false" autocomplete="off" />
      {#if query}
        <button class="clear" aria-label="Clear search" onclick={() => (query = '')}><X size={14} strokeWidth={2} /></button>
      {/if}
    </label>
  </header>

  {#if tags.length > 0}
    <div class="filters" role="radiogroup" aria-label="Filter by type">
      {#each [undefined, ...tags] as t (t ?? 'all')}
        <button role="radio" aria-checked={tag === t} class:on={tag === t} onclick={() => (tag = t)}>{t ?? 'All'}</button>
      {/each}
    </div>
  {/if}

  {#if store.servers.status === 'loading'}
    <div class="grid" aria-busy="true" aria-label="Loading servers">
      <div class="placeholder wide"></div>
      {#each { length: 4 } as _, i (i)}<div class="placeholder"></div>{/each}
    </div>
  {:else if store.servers.status === 'error'}
    <InlineState message={store.offline ? 'You’re offline. Servers load when you reconnect.' : 'Featured servers didn’t load.'} onretry={() => void store.reloadServers()} />
  {:else if all.length === 0}
    <div class="first-run">
      <h2>Servers appear after your first game</h2>
      <p>Cinnabar lists featured servers once the game has signed you in. Play once, and they show up here.</p>
      <button class="first-play" onclick={() => void store.play()}>Play Cinnabar</button>
    </div>
  {:else if results.length === 0}
    <div class="none">
      <p>No servers match {query.trim() ? `“${query.trim()}”` : 'this filter'}.</p>
      <button class="text-button" onclick={() => ((query = ''), (tag = undefined))}>Show all servers</button>
    </div>
  {:else}
    <p class="visually-hidden" aria-live="polite">{results.length} servers</p>
    <div class="grid">
      {#if spotlight}
        <article class="spotlight">
          <div class="art"><ServerArt name={spotlight.name} imageUrl={spotlight.imageUrl} /></div>
          <div class="copy">
            <h2>{spotlight.name}</h2>
            {#if spotlight.motd ?? spotlight.description}<p class="motd">{spotlight.motd ?? spotlight.description}</p>{/if}
            {#if spotlight.motd && spotlight.description}<p class="desc">{spotlight.description}</p>{/if}
            <p class="facts">
              <OnlineCount online={spotlight.online} />
              {#if spotlight.pingMs !== undefined}<Ping ms={spotlight.pingMs} />{/if}
              {#if spotlight.tags?.length}<span class="tags">{spotlight.tags.join(', ')}</span>{/if}
            </p>
            <div class="actions">
              <ServerPlay server={spotlight} emphasis />
              {#if isExperience(spotlight.port)}
                <span class="address">Experience</span>
              {:else}
                <button class="address" title="Copy address" onclick={() => copyAddress(spotlight)}>
                  {formatAddress(spotlight.address, spotlight.port)}
                  {#if copied === spotlight.id}<Check size={13} strokeWidth={2.5} aria-label="Copied" />{:else}<Copy size={13} strokeWidth={2} aria-hidden="true" />{/if}
                </button>
              {/if}
            </div>
          </div>
        </article>
      {/if}
      {#each rest as server (server.id)}
        <ServerCard {server} variant="card" />
      {/each}
    </div>
  {/if}
</div>

<style>
  .page {
    max-width: 1240px;
    padding: 20px 40px 40px;
  }

  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--s-5);
  }

  h1 {
    font: var(--t-page);
    letter-spacing: var(--track-display);
  }

  .lede {
    margin-top: 8px;
    color: var(--ink-2);
  }

  .search {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    width: min(320px, 45%);
    height: 42px;
    padding: 0 8px 0 14px;
    border-radius: 999px;
    background: var(--glass);
    -webkit-backdrop-filter: blur(16px) saturate(1.4);
    backdrop-filter: blur(16px) saturate(1.4);
    box-shadow: inset 0 0 0 1px var(--glass-edge);
    color: var(--ink-3);
  }

  .search:focus-within {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    outline: none;
    color: var(--ink);
    user-select: text;
    -webkit-user-select: text;
  }

  .search input::-webkit-search-cancel-button {
    display: none;
  }

  .clear {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    color: var(--ink-2);
  }

  .clear:hover {
    background: var(--raise-2);
  }

  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 24px;
  }

  .filters button {
    height: 32px;
    padding: 0 14px;
    border-radius: 999px;
    background: var(--raise);
    font: var(--t-label);
    color: var(--ink-2);
    transition:
      background-color var(--t-fast) linear,
      color var(--t-fast) linear;
  }

  .filters button:hover {
    color: var(--ink);
    background: var(--raise-2);
  }

  .filters .on {
    background: var(--ink);
    color: var(--solid);
  }

  .filters .on:hover {
    background: var(--ink);
    color: var(--solid);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 20px;
    margin-top: 24px;
  }

  .spotlight {
    grid-column: 1 / -1;
    display: grid;
    grid-template-columns: minmax(0, 1.1fr) minmax(0, 1fr);
    min-height: 230px;
    border-radius: var(--r-lg);
    overflow: hidden;
    background: var(--glass-strong);
    -webkit-backdrop-filter: blur(24px) saturate(1.4);
    backdrop-filter: blur(24px) saturate(1.4);
    box-shadow: inset 0 0 0 1px var(--glass-edge);
  }

  .spotlight .art {
    min-height: 100%;
  }

  .spotlight .copy {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 28px 30px;
  }

  .spotlight h2 {
    font: 700 28px / 1.05 var(--font);
    letter-spacing: var(--track-display);
  }

  .motd {
    font: var(--t-label);
    font-size: 15px;
    color: var(--ink);
  }

  .desc {
    color: var(--ink-2);
    font-size: 14px;
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
    margin-top: auto;
    font: var(--t-meta);
    font-size: 13px;
  }

  .tags {
    color: var(--ink-3);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: 8px;
  }

  .address {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font: var(--t-meta);
    font-size: 13px;
    color: var(--ink-3);
  }

  .address:hover {
    color: var(--ink);
  }

  .placeholder {
    aspect-ratio: 16 / 10;
    border-radius: var(--r-lg);
    background: var(--raise);
  }

  .placeholder.wide {
    grid-column: 1 / -1;
    aspect-ratio: auto;
    height: 260px;
  }

  .first-run {
    max-width: 460px;
    margin-top: 48px;
  }

  .first-run h2 {
    font: var(--t-title);
    font-size: 22px;
  }

  .first-run p {
    margin-top: 8px;
    color: var(--ink-2);
  }

  .first-play {
    height: 40px;
    margin-top: 18px;
    padding: 0 18px;
    border-radius: 999px;
    background: var(--cinnabar-fill);
    color: var(--cinnabar-ink);
    font: var(--t-label);
    font-size: 14px;
  }

  .first-play:hover {
    background: var(--cinnabar);
  }

  .none {
    display: flex;
    align-items: center;
    gap: var(--s-4);
    margin-top: var(--s-6);
    color: var(--ink-2);
  }

  .text-button {
    color: var(--ink);
    font-weight: 600;
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  @media (max-width: 1000px) {
    .grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .spotlight {
      grid-template-columns: minmax(0, 0.8fr) minmax(0, 1fr);
      min-height: 210px;
    }

    .spotlight .copy {
      padding: 22px 24px;
    }
  }
</style>
