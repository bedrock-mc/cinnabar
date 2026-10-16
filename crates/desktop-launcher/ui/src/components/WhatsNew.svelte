<script lang="ts">
  import ArrowUpRight from '@lucide/svelte/icons/arrow-up-right';
  import type { Changelog } from '../api';
  import { store } from '../lib/store.svelte';

  let { log }: { log: Changelog } = $props();
</script>

<!-- The installed build's notes; on its first launch the card carries the moment. -->
<article class="whatsnew" class:unseen={log.unseen}>
  <h2>
    {#if log.unseen}<span class="spark" aria-hidden="true"></span>{/if}
    What’s new in {log.version}
  </h2>
  <ul>
    {#each log.highlights.slice(0, 3) as line (line)}<li>{line}</li>{/each}
  </ul>
  {#if log.url}
    <button class="more" onclick={() => void store.open(log.url!)}>Full changelog<ArrowUpRight size={13} strokeWidth={2} aria-hidden="true" /></button>
  {/if}
</article>

<style>
  .whatsnew {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 18px 20px 16px;
    border-radius: var(--r-lg);
    background:
      radial-gradient(140% 100% at 0% 0%, rgb(226 72 58 / 0.22), transparent 60%),
      var(--glass);
    -webkit-backdrop-filter: blur(24px) saturate(1.5);
    backdrop-filter: blur(24px) saturate(1.5);
    box-shadow: inset 0 0 0 1px var(--glass-edge);
  }

  .unseen {
    box-shadow:
      inset 0 0 0 1px rgb(242 122 99 / 0.55),
      0 14px 40px -18px rgb(226 72 58 / 0.9);
  }

  h2 {
    display: flex;
    align-items: center;
    gap: 8px;
    font: var(--t-title);
    letter-spacing: var(--track-title);
  }

  .spark {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    background: var(--cinnabar-hi);
    transform: rotate(45deg);
    box-shadow: 0 0 0 4px rgb(242 122 99 / 0.2);
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 7px;
    margin: 12px 0 0;
    padding: 0;
    list-style: none;
  }

  li {
    position: relative;
    padding-left: 14px;
    font: var(--t-meta);
    font-size: 13px;
    color: var(--ink-2);
  }

  li::before {
    content: '';
    position: absolute;
    left: 0;
    top: 0.55em;
    width: 5px;
    height: 5px;
    border-radius: 1px;
    background: var(--cinnabar);
    transform: rotate(45deg);
  }

  .more {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    align-self: flex-start;
    margin-top: auto;
    font: var(--t-meta);
    font-weight: 600;
    color: var(--ink-2);
  }

  .more:hover {
    color: var(--ink);
  }
</style>
