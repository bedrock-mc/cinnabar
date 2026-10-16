<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import type { Primary } from '../lib/play';

  interface Props {
    primary: Primary;
    onactivate: () => void;
  }

  let { primary, onactivate }: Props = $props();

  const percent = $derived(typeof primary.progress === 'number' ? Math.floor(primary.progress * 100) : undefined);
  const indeterminate = $derived(primary.tone === 'busy' && percent === undefined);
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const enter = (node: Element) => fly(node, { y: reduce ? 0 : 8, duration: reduce ? 0 : 320, easing: cubicOut });
  const leave = (node: Element) => fade(node, { duration: reduce ? 0 : 120 });
</script>

<button class="play" style:--mask={percent === undefined ? 1 : percent / 100} data-tone={primary.tone} disabled={primary.intent === 'none'} aria-busy={primary.tone === 'busy'} onclick={onactivate}>
  {#if primary.tone === 'busy'}
    <span class="fill" class:indeterminate style:--p={percent === undefined ? 1 : percent / 100}></span>
  {/if}
  <span class="sheen" aria-hidden="true"></span>
  <span class="stack">
    {#key primary.label}
      <span class="content" in:enter out:leave>
        <span class="label">
          {#if primary.tone === 'running'}<span class="live" aria-hidden="true"></span>{/if}
          {primary.label}
        </span>
        {#if primary.detail}<span class="detail">{primary.detail}</span>{/if}
      </span>
    {/key}
  </span>
  {#if percent !== undefined}<span class="pct">{percent}<small>%</small></span>{/if}
</button>

<style>
  .play {
    position: relative;
    isolation: isolate;
    display: flex;
    align-items: center;
    width: 300px;
    height: 62px;
    padding: 0 22px;
    border-radius: 14px;
    overflow: hidden;
    text-align: left;
    color: var(--cinnabar-ink);
    background: var(--glass-strong);
    box-shadow: inset 0 0 0 1px var(--glass-edge);
    transition:
      transform var(--t-med) var(--ease-out),
      box-shadow var(--t-med) var(--ease-out),
      background-color var(--t-fast) linear;
  }

  /* The ready state is a lit block face: brighter at the top edge, deepening toward the base. */
  .play[data-tone='ore'] {
    background: linear-gradient(180deg, #ec5544 0%, var(--cinnabar-fill) 52%, #bd3228 100%);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.3),
      inset 0 -2px 0 rgb(0 0 0 / 0.18),
      0 12px 32px -12px rgb(210 59 46 / 0.85);
  }

  .play[data-tone='ore']:hover {
    transform: translateY(-2px);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.34),
      inset 0 -2px 0 rgb(0 0 0 / 0.18),
      0 18px 40px -12px rgb(226 72 58 / 0.95);
  }

  .play[data-tone='ore']:active {
    transform: translateY(1px) scale(0.99);
    transition-duration: 60ms;
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 0.2),
      inset 0 2px 6px rgb(0 0 0 / 0.25),
      0 6px 18px -10px rgb(210 59 46 / 0.8);
  }

  /* A single band of light crosses the face on hover, and loops across progress while busy. */
  .sheen {
    position: absolute;
    inset: 0;
    z-index: -1;
    background: linear-gradient(105deg, transparent 35%, rgb(255 255 255 / 0.22) 50%, transparent 65%);
    background-size: 250% 100%;
    background-position: 130% 0;
    pointer-events: none;
  }

  .play[data-tone='ore']:hover .sheen {
    animation: sweep 900ms var(--ease-out);
  }

  .play[data-tone='busy'] .sheen {
    animation: sweep 1.8s var(--ease-in-out) infinite;
    mask: linear-gradient(90deg, #000 calc(var(--mask, 1) * 100%), transparent 0);
  }

  @keyframes sweep {
    from {
      background-position: 130% 0;
    }
    to {
      background-position: -30% 0;
    }
  }

  .fill {
    position: absolute;
    inset: 0;
    z-index: -2;
    background: linear-gradient(180deg, #e04b3b, #b8302a);
    transform-origin: left;
    transform: scaleX(var(--p));
    transition: transform 500ms var(--ease-out);
  }

  .fill.indeterminate {
    width: 30%;
    transform: none;
    animation: travel 1.6s var(--ease-in-out) infinite alternate;
  }

  @keyframes travel {
    to {
      translate: calc(100% / 0.3 - 100%) 0;
    }
  }

  .stack {
    display: grid;
    flex: 1;
    min-width: 0;
  }

  .content {
    grid-area: 1 / 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }

  .label {
    display: flex;
    align-items: center;
    gap: 10px;
    font: 680 20px / 1.05 var(--font);
    letter-spacing: -0.02em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .detail {
    font: var(--t-meta);
    opacity: 0.82;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .pct {
    flex: none;
    margin-left: 16px;
    font: 680 22px / 1 var(--font);
    letter-spacing: -0.02em;
    font-variant-numeric: tabular-nums;
  }

  .pct small {
    font-size: 13px;
    margin-left: 1px;
    opacity: 0.8;
  }

  .play[data-tone='running'],
  .play[data-tone='disabled'] {
    color: var(--ink);
  }

  .play[data-tone='disabled'] {
    color: var(--ink-3);
  }

  .live {
    position: relative;
    width: 10px;
    height: 10px;
    border-radius: 3px;
    background: var(--ok);
  }

  .live::after {
    content: '';
    position: absolute;
    inset: -4px;
    border-radius: 6px;
    border: 2px solid var(--ok);
    opacity: 0;
    animation: ping 2.4s var(--ease-out) infinite;
  }

  @keyframes ping {
    0% {
      opacity: 0.7;
      transform: scale(0.6);
    }
    100% {
      opacity: 0;
      transform: scale(1.4);
    }
  }

  .play:focus-visible {
    outline-offset: 4px;
  }

  @media (prefers-reduced-motion: reduce) {
    .play,
    .fill {
      transition: none;
    }

    .play[data-tone='ore']:hover,
    .play[data-tone='ore']:active {
      transform: none;
    }

    .sheen,
    .play[data-tone='ore']:hover .sheen,
    .play[data-tone='busy'] .sheen,
    .live::after {
      animation: none;
    }

    .fill.indeterminate {
      width: 100%;
      animation: none;
      opacity: 0.4;
    }
  }
</style>
