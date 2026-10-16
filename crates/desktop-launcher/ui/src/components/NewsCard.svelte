<script lang="ts">
  import type { NewsItem } from '../api';
  import { store } from '../lib/store.svelte';
  import { formatDate } from '../lib/format';
  import { newsPoster } from '../lib/poster';

  let { item }: { item: NewsItem } = $props();
  let broken = $state(false);
  const poster = $derived(newsPoster(item.title, item.tag));
</script>

<button class="card" onclick={() => void store.open(item.url)}>
  <span class="art" style:--h={poster.hue}>
    {#if item.imageUrl && !broken}
      <img src={item.imageUrl} alt="" loading="lazy" onerror={() => (broken = true)} />
    {:else}
      <span class="poster" aria-hidden="true">{poster.text}</span>
    {/if}
  </span>
  <span class="meta">
    {#if item.tag}<span class="tag">{item.tag}</span>{/if}
    <time datetime={item.publishedAt}>{formatDate(item.publishedAt)}</time>
  </span>
  <span class="title">{item.title}</span>
</button>

<style>
  .card {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    padding: 8px 8px 12px;
    border-radius: var(--r-lg);
    background: var(--glass);
    -webkit-backdrop-filter: blur(24px) saturate(1.5);
    backdrop-filter: blur(24px) saturate(1.5);
    box-shadow: inset 0 0 0 1px var(--glass-edge);
    text-align: left;
    transition:
      transform var(--t-med) var(--ease-out),
      background-color var(--t-fast) linear;
  }

  .card:hover {
    transform: translateY(-2px);
    background: var(--glass-strong);
  }

  .art {
    position: relative;
    display: block;
    flex: 1;
    min-height: 0;
    border-radius: 11px;
    overflow: hidden;
    container-type: size;
    background:
      radial-gradient(120% 90% at 12% 0%, rgb(255 255 255 / 0.2), transparent 60%),
      linear-gradient(150deg, hsl(var(--h) 52% 38%), hsl(var(--h) 48% 18%));
  }

  img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    transition: transform var(--t-slow) var(--ease-out);
  }

  .card:hover img {
    transform: scale(1.04);
  }

  .poster {
    position: absolute;
    left: 12px;
    bottom: -0.16em;
    font: 760 64px / 1 var(--font);
    font-size: min(62cqh, 30cqw);
    letter-spacing: -0.045em;
    color: hsl(var(--h) 70% 76%);
    opacity: 0.85;
    white-space: nowrap;
  }

  .meta {
    display: flex;
    gap: 8px;
    margin: 10px 6px 0;
    font: var(--t-meta);
    font-size: 11px;
    color: var(--ink-3);
  }

  .tag {
    color: var(--cinnabar-hi);
    font-weight: 650;
  }

  .title {
    margin: 3px 6px 0;
    font: var(--t-label);
    color: var(--ink);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  @media (prefers-reduced-motion: reduce) {
    .card,
    img {
      transition: none;
    }

    .card:hover {
      transform: none;
    }
  }
</style>
