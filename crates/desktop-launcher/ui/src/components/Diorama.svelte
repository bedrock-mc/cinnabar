<script lang="ts">
  import { onMount } from 'svelte';
  import type { Diorama, Frame } from '../lib/diorama/scene';
  import { MOODS, moodForHour, type Mood } from '../lib/diorama/mood';

  interface Props {
    /** Fraction of the cinnabar vein that glows. */
    refined: number;
    /** Stop rendering, e.g. while the game runs. */
    paused: boolean;
    /** Where the island sits within the canvas. */
    frame: Frame;
    /** Blur and dim for pages that sit over the world. */
    veiled?: boolean;
    orbit?: number;
  }

  let { refined, paused, frame, veiled = false, orbit }: Props = $props();

  const forced = new URLSearchParams(location.search).get('mood');
  const mood: Mood = forced && forced in MOODS ? (forced as Mood) : moodForHour(new Date().getHours());
  const m = MOODS[mood];
  const hex = (n: number) => `#${n.toString(16).padStart(6, '0')}`;

  let canvas = $state<HTMLCanvasElement>();
  let scene = $state<Diorama>();
  let failed = $state(false);
  let ready = $state(false);

  onMount(() => {
    const still = matchMedia('(prefers-reduced-motion: reduce)').matches || new URLSearchParams(location.search).has('still');
    let disposed = false;
    let instance: Diorama | undefined;
    const observer = new ResizeObserver(() => instance?.resize());
    const onPointer = (e: PointerEvent) => instance?.pointer((e.clientX / innerWidth) * 2 - 1, (e.clientY / innerHeight) * 2 - 1);

    import('../lib/diorama/scene')
      .then(({ createDiorama }) => {
        if (disposed) return;
        instance = createDiorama(canvas!, { mood, frame, still, orbit });
        scene = instance;
        observer.observe(canvas!);
        if (!still) addEventListener('pointermove', onPointer, { passive: true });
        requestAnimationFrame(() => (ready = true));
      })
      .catch((err: unknown) => {
        console.error('diorama unavailable', err);
        failed = true;
      });

    return () => {
      disposed = true;
      observer.disconnect();
      removeEventListener('pointermove', onPointer);
      instance?.dispose();
    };
  });

  $effect(() => scene?.setRefined(refined));
  $effect(() => scene?.setPaused(paused));
  $effect(() => scene?.setFrame(frame));
</script>

<div
  class="diorama"
  class:ready
  class:veiled
  style:--sky-top={hex(m.skyTop)}
  style:--sky-horizon={hex(m.horizon)}
  aria-hidden="true"
>
  {#if failed}
    <!-- Without WebGL: the island and its vein drawn in the active mood's palette. -->
    <svg class="still" viewBox="0 0 160 90" preserveAspectRatio="xMidYMid slice">
      <defs>
        <radialGradient id="still-sun" cx={m.sunScreen[0]} cy={1 - m.sunScreen[1]} r="0.35">
          <stop offset="0" stop-color={hex(m.sunGlow)} stop-opacity="0.7" />
          <stop offset="1" stop-color={hex(m.sunGlow)} stop-opacity="0" />
        </radialGradient>
      </defs>
      <rect width="160" height="90" fill="url(#still-sun)" />
      <path
        d="M96 34h8v-4h12v-3h10v4h8v5h6v14h-4v8h-6v6h-8v5h-12v-4h-8v-6h-6v-8h-4v-12h4z"
        fill={hex(m.hemiGround)}
      />
      <path d="M96 34h8v-4h12v-3h10v4h8v5h6v3h-44z" fill={hex(m.hemiSky)} opacity="0.55" />
      <path d="M114 31h5v12h3v10h3v14h-5v-10h-3v-10h-3z" fill={hex(m.sunGlow)} opacity="0.9" />
    </svg>
  {:else}
    <canvas bind:this={canvas}></canvas>
  {/if}
</div>

<style>
  .diorama {
    position: absolute;
    inset: 0;
    overflow: hidden;
    /* The sky's own gradient shows while the scene loads and if WebGL is unavailable. */
    background: linear-gradient(to bottom, var(--sky-top), var(--sky-horizon));
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
    opacity: 0;
    transition:
      opacity 900ms var(--ease-out),
      filter 500ms var(--ease-out),
      transform 500ms var(--ease-out);
  }

  .still {
    display: block;
    width: 100%;
    height: 100%;
    transition: filter 500ms var(--ease-out);
  }

  .veiled .still {
    filter: blur(18px) brightness(0.55);
  }

  .ready canvas {
    opacity: 1;
  }

  .veiled canvas {
    filter: blur(18px) saturate(1.1) brightness(0.55);
    transform: scale(1.06);
  }

  @media (prefers-reduced-motion: reduce) {
    canvas {
      transition: none;
    }
  }
</style>
