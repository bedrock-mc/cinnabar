<script lang="ts">
  import { onMount } from 'svelte';
  import TitleBar from './components/TitleBar.svelte';
  import Diorama from './components/Diorama.svelte';
  import Home from './pages/Home.svelte';
  import Servers from './pages/Servers.svelte';
  import Settings from './pages/Settings.svelte';
  import { store } from './lib/store.svelte';
  import { playModel, veinFill } from './lib/play';
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';

  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const enter = (node: Element) => fly(node, { y: reduce ? 0 : 10, duration: reduce ? 0 : 360, easing: cubicOut, opacity: reduce ? 1 : 0 });

  onMount(() => store.start());

  // `?view=diorama` renders the world alone, for stills and generated art.
  const q = new URLSearchParams(location.search);
  const probe = q.get('view') === 'diorama';
  const probeFrame = {
    x: Number(q.get('fx') ?? 0),
    y: Number(q.get('fy') ?? 0),
    zoom: Number(q.get('zoom') ?? 1),
    figure: q.has('px') ? { x: Number(q.get('px')), y: Number(q.get('py') ?? 0.7) } : undefined,
  };
  const probeOrbit = q.has('orbit') ? Number(q.get('orbit')) : undefined;

  // The island sits right of centre; the player stands just right of the launch column with their
  // feet on the dock's top edge, at every window size.
  let width = $state(innerWidth);
  let height = $state(innerHeight);
  const frame = $derived({
    x: 0.27,
    y: 0.1,
    zoom: 0.84,
    figure: { x: Math.max(0.56, 690 / width), y: 1 - (height < 640 ? 214 : 246) / height },
  });

  const install = $derived(store.install.status === 'ready' ? store.install.value : undefined);
  // On the first launch of a new version the vein starts dark and re-ignites: the "what's new" moment.
  let ignited = $state(true);
  let celebrated = false;
  $effect(() => {
    if (celebrated || !store.changelog?.unseen) return;
    celebrated = true;
    ignited = false;
    setTimeout(() => (ignited = true), 700);
  });
  const refined = $derived(ignited ? veinFill(playModel(install, store.install.status === 'error', store.update, store.game, store.now)) : 0);
  const home = $derived(store.page === 'home');

  // Native keyboard affordance: Mod+1..3 switch pages like tabs.
  function onKey(e: KeyboardEvent) {
    if (!(e.metaKey || e.ctrlKey) || e.altKey || e.shiftKey) return;
    const page = ({ '1': 'home', '2': 'servers', '3': 'settings' } as const)[e.key as '1' | '2' | '3'];
    if (page) {
      e.preventDefault();
      store.navigate(page);
    }
  }
</script>

<svelte:window onkeydown={onKey} bind:innerWidth={width} bind:innerHeight={height} />

{#if probe}
  <div class="probe"><Diorama refined={1} paused={false} frame={probeFrame} orbit={probeOrbit} /></div>
{:else}
<div class="frame" data-platform={store.platform} class:home>
  <Diorama {refined} {frame} veiled={!home} paused={!home || store.game?.kind === 'running'} />
  <div class="scrim" aria-hidden="true"></div>
  <TitleBar />
  <main id="main" tabindex="-1">
    {#key store.page}
      <div class="page" in:enter>
        {#if store.page === 'servers'}
          <Servers />
        {:else if store.page === 'settings'}
          <Settings />
        {:else}
          <Home />
        {/if}
      </div>
    {/key}
  </main>
</div>
{/if}

<style>
  .probe {
    position: fixed;
    inset: 0;
  }

  .frame {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  /* Legibility comes from shaping the light, not boxing the text. */
  .scrim {
    position: absolute;
    inset: 0;
    pointer-events: none;
    transition: background-color var(--t-slow) var(--ease-out);
    background:
      linear-gradient(180deg, rgb(8 9 16 / 0.55) 0%, transparent 16%),
      linear-gradient(90deg, rgb(8 9 16 / 0.82) 0%, rgb(8 9 16 / 0.45) 34%, transparent 58%),
      linear-gradient(0deg, rgb(8 9 16 / 0.6) 0%, transparent 34%);
  }

  .frame:not(.home) .scrim {
    background: rgb(8 9 16 / 0.45);
  }

  .page {
    height: 100%;
  }

  main {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    outline: none;
  }
</style>
