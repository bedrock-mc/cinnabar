<script lang="ts">
  import ServerArt from './ServerArt.svelte';
  import ServerPlay from './ServerPlay.svelte';
  import OnlineCount from './OnlineCount.svelte';
  import Ping from './Ping.svelte';
  import type { Server } from '../api/proposed';

  interface Props {
    server: Server;
    /** `tile` is a compact row; `card` is banner art with details revealed on hover or focus. */
    variant: 'tile' | 'card';
  }

  let { server, variant }: Props = $props();
</script>

<article class="server" data-variant={variant}>
  <div class="art"><ServerArt name={server.name} imageUrl={server.imageUrl} /></div>
  <div class="body">
    <h3>{server.name}</h3>
    <p class="facts">
      <OnlineCount online={server.online} />
      {#if server.pingMs !== undefined}<Ping ms={server.pingMs} />{/if}
    </p>
    {#if variant === 'card'}
      <div class="reveal">
        {#if server.motd ?? server.description}<p class="motd">{server.motd ?? server.description}</p>{/if}
        {#if server.tags?.length}
          <ul class="tags">{#each server.tags as tag (tag)}<li>{tag}</li>{/each}</ul>
        {/if}
      </div>
    {:else if server.motd ?? server.description}
      <p class="motd">{server.motd ?? server.description}</p>
    {/if}
  </div>
  <div class="join"><ServerPlay {server} /></div>
</article>

<style>
  .server {
    position: relative;
    min-width: 0;
  }

  h3 {
    font: var(--t-label);
    font-size: 15px;
    font-weight: 700;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .facts {
    display: flex;
    gap: 14px;
    font: var(--t-meta);
  }

  .motd {
    font: var(--t-meta);
    font-size: 12.5px;
    color: var(--ink-2);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  /* Tile: art square, two text lines, Play at the end. */
  [data-variant='tile'] {
    display: grid;
    grid-template-columns: 52px 1fr auto;
    align-items: center;
    gap: 14px;
  }

  [data-variant='tile'] .art {
    width: 52px;
    height: 52px;
    border-radius: var(--r-md);
    overflow: hidden;
  }

  [data-variant='tile'] .body {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }

  /* Card: banner art carries the identity; details rise over it on hover or keyboard focus. */
  [data-variant='card'] {
    aspect-ratio: 16 / 10;
    border-radius: var(--r-lg);
    overflow: hidden;
    box-shadow: inset 0 0 0 1px var(--glass-edge);
  }

  [data-variant='card'] .art {
    position: absolute;
    inset: 0;
    transition: transform var(--t-slow) var(--ease-out);
  }

  [data-variant='card']::after {
    content: '';
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, transparent 30%, rgb(8 9 16 / 0.86) 100%);
    pointer-events: none;
  }

  [data-variant='card'] .body {
    position: absolute;
    z-index: 1;
    left: 16px;
    right: 16px;
    bottom: 14px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  [data-variant='card'] h3 {
    font-size: 17px;
  }

  /* Small cards keep one fact per line rather than wrapping mid-phrase. */
  [data-variant='card'] .facts {
    flex-wrap: wrap;
    row-gap: 2px;
    white-space: nowrap;
  }

  .reveal {
    display: grid;
    grid-template-rows: 0fr;
    opacity: 0;
    transition:
      grid-template-rows var(--t-med) var(--ease-out),
      opacity var(--t-med) linear;
  }

  .reveal > * {
    min-height: 0;
  }

  .reveal {
    overflow: hidden;
  }

  [data-variant='card']:hover .reveal,
  [data-variant='card']:focus-within .reveal {
    grid-template-rows: 1fr;
    opacity: 1;
  }

  [data-variant='card']:hover .art,
  [data-variant='card']:focus-within .art {
    transform: scale(1.05);
  }

  .tags {
    display: flex;
    gap: 6px;
    margin: 6px 0 0;
    padding: 0;
    list-style: none;
  }

  .tags li {
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--raise-2);
    font: var(--t-meta);
    font-size: 11px;
    color: var(--ink);
  }

  [data-variant='card'] .join {
    position: absolute;
    z-index: 1;
    top: 12px;
    right: 12px;
    opacity: 0;
    transform: translateY(-4px);
    transition:
      opacity var(--t-fast) linear,
      transform var(--t-med) var(--ease-out);
  }

  [data-variant='card']:hover .join,
  [data-variant='card']:focus-within .join {
    opacity: 1;
    transform: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .art,
    .reveal,
    .join {
      transition: none !important;
    }

    [data-variant='card']:hover .art {
      transform: none;
    }
  }
</style>
