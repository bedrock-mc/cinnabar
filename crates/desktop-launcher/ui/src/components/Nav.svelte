<script lang="ts">
  import House from '@lucide/svelte/icons/house';
  import Globe from '@lucide/svelte/icons/globe';
  import Settings from '@lucide/svelte/icons/settings-2';
  import { store, type Page } from '../lib/store.svelte';

  interface Props {
    variant: 'tabs' | 'pill' | 'list';
  }

  let { variant }: Props = $props();

  const items: { page: Page; label: string; icon: typeof House; key: string }[] = [
    { page: 'home', label: 'Home', icon: House, key: '1' },
    { page: 'servers', label: 'Servers', icon: Globe, key: '2' },
    { page: 'settings', label: 'Settings', icon: Settings, key: '3' },
  ];

  const els: HTMLAnchorElement[] = $state([]);
  let indicator = $state({ x: 0, y: 0, w: 0, h: 0, ready: false });
  const mod = /Mac/.test(navigator.userAgent) ? '⌘' : 'Ctrl+';

  // The active marker slides between items rather than jumping, so the change of place is visible.
  $effect(() => {
    const el = els[items.findIndex((i) => i.page === store.page)];
    if (!el) return;
    const measure = () => (indicator = { x: el.offsetLeft, y: el.offsetTop, w: el.offsetWidth, h: el.offsetHeight, ready: true });
    measure();
    document.fonts.ready.then(measure);
  });
</script>

<nav class="nav" data-variant={variant} aria-label="Main">
  <span
    class="marker"
    class:ready={indicator.ready}
    style:transform="translate({indicator.x}px, {indicator.y}px)"
    style:width="{indicator.w}px"
    style:height="{indicator.h}px"
    aria-hidden="true"
  ></span>
  {#each items as item, i (item.page)}
    <a
      bind:this={els[i]}
      href={item.page === 'home' ? '#' : `#/${item.page}`}
      aria-current={store.page === item.page ? 'page' : undefined}
      title="{item.label} ({mod}{item.key})"
      onclick={(e) => {
        e.preventDefault();
        store.navigate(item.page);
      }}
    >
      {#if variant === 'list'}<item.icon size={18} strokeWidth={1.9} aria-hidden="true" />{/if}
      {item.label}
    </a>
  {/each}
</nav>

<style>
  .nav {
    position: relative;
    display: flex;
    align-items: center;
  }

  a {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 12px;
    font: var(--t-label);
    font-size: 14px;
    color: var(--ink-2);
    text-decoration: none;
    white-space: nowrap;
    transition: color var(--t-fast) linear;
  }

  a:hover,
  a[aria-current='page'] {
    color: var(--ink);
  }

  .marker {
    position: absolute;
    top: 0;
    left: 0;
    opacity: 0;
    pointer-events: none;
  }

  .marker.ready {
    opacity: 1;
    transition:
      transform var(--t-slow) var(--ease-out),
      width var(--t-slow) var(--ease-out);
  }

  /* Tabs: plain words with a short cinnabar bar under the current one. */
  [data-variant='tabs'] {
    gap: 28px;
    height: 100%;
  }

  [data-variant='tabs'] a {
    height: 100%;
  }

  [data-variant='tabs'] .marker::after {
    content: '';
    position: absolute;
    left: 50%;
    bottom: 6px;
    width: 18px;
    height: 3px;
    margin-left: -9px;
    border-radius: 2px;
    background: var(--cinnabar);
  }

  /* Pill: a segmented control with a glass thumb. */
  [data-variant='pill'] {
    gap: 2px;
    padding: 3px;
    border-radius: 999px;
    background: var(--glass);
    -webkit-backdrop-filter: blur(16px) saturate(1.4);
    backdrop-filter: blur(16px) saturate(1.4);
    box-shadow: inset 0 0 0 1px var(--glass-edge);
  }

  [data-variant='pill'] a {
    height: 30px;
    padding: 0 16px;
    border-radius: 999px;
  }

  [data-variant='pill'] .marker {
    border-radius: 999px;
    background: var(--raise-2);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 0.08);
  }

  /* List: a vertical rail of labelled rows. */
  [data-variant='list'] {
    flex-direction: column;
    align-items: stretch;
    gap: 2px;
  }

  [data-variant='list'] a {
    height: 40px;
    padding: 0 12px;
    border-radius: 10px;
  }

  [data-variant='list'] .marker {
    border-radius: 10px;
    background: var(--raise);
  }

  [data-variant='list'] .marker::before {
    content: '';
    position: absolute;
    left: -12px;
    top: 12px;
    bottom: 12px;
    width: 3px;
    border-radius: 2px;
    background: var(--cinnabar);
  }

  @media (prefers-reduced-motion: reduce) {
    .marker.ready {
      transition: none;
    }
  }
</style>
