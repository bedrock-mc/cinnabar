<script lang="ts">
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import ServerArt from './ServerArt.svelte';
  import ServerPlay from './ServerPlay.svelte';
  import { store } from '../lib/store.svelte';
  import { formatAgo, formatCount } from '../lib/format';

  // Recent joins come first; curated servers fill in for players who haven't joined any yet.
  const recent = $derived(store.recent.slice(0, 3));
  const featured = $derived(store.servers.status === 'ready' ? store.servers.value.slice(0, 3) : []);
  const rows = $derived(
    recent.length > 0
      ? recent.map((s) => ({ key: `${s.address}:${s.port}`, name: s.name, imageUrl: s.imageUrl, meta: formatAgo(s.lastPlayedAt, store.now), server: { ...s, id: s.address } }))
      : featured.map((s) => ({ key: s.id, name: s.name, imageUrl: s.imageUrl, meta: s.online !== undefined ? `${formatCount(s.online)} online` : (s.tags?.[0] ?? ''), server: s })),
  );
</script>

<section class="list" class:hint={rows.length === 0} aria-labelledby="servers-title">
    <header>
      <h2 id="servers-title">{recent.length > 0 ? 'Jump back in' : rows.length > 0 ? 'Featured servers' : 'Servers'}</h2>
      <a href="#/servers" onclick={(e) => { e.preventDefault(); store.navigate('servers'); }}>
        Servers<ChevronRight size={14} strokeWidth={2} aria-hidden="true" />
      </a>
    </header>
    {#if rows.length === 0}
      <p class="first-run">Servers you join show up here for one-click rejoin. Featured servers appear after your first game.</p>
    {/if}
    <ul>
      {#each rows as row (row.key)}
        <li>
          <span class="thumb"><ServerArt name={row.name} imageUrl={row.imageUrl} /></span>
          <span class="copy"><span class="name">{row.name}</span><span class="meta">{row.meta}</span></span>
          <ServerPlay server={row.server} />
        </li>
      {/each}
    </ul>
  </section>

<style>
  .list {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 16px 16px 10px 18px;
    border-radius: var(--r-lg);
    background: var(--glass);
    -webkit-backdrop-filter: blur(24px) saturate(1.5);
    backdrop-filter: blur(24px) saturate(1.5);
    box-shadow: inset 0 0 0 1px var(--glass-edge);
  }

  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  h2 {
    font: var(--t-title);
    letter-spacing: var(--track-title);
  }

  a {
    display: inline-flex;
    align-items: center;
    font: var(--t-meta);
    font-weight: 600;
    color: var(--ink-3);
    text-decoration: none;
  }

  a:hover {
    color: var(--ink);
  }

  ul {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: 34px 1fr auto;
    align-items: center;
    gap: 12px;
    padding: 6px 0;
  }

  li + li {
    box-shadow: inset 0 1px 0 var(--line);
  }

  .hint {
    height: auto;
    padding-bottom: 18px;
  }

  .first-run {
    max-width: 34ch;
    margin-top: 4px;
    font: var(--t-meta);
    font-size: 13px;
    color: var(--ink-2);
  }

  .thumb {
    width: 34px;
    height: 34px;
    border-radius: 9px;
    overflow: hidden;
  }

  .copy {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .name {
    font: var(--t-label);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .meta {
    font: var(--t-meta);
    font-size: 11px;
    color: var(--ink-3);
  }

  li :global(.join) {
    height: 28px;
    padding: 0 11px;
    font-size: 12px;
  }
</style>
