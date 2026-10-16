<script lang="ts">
  let { ms }: { ms: number } = $props();
  // Bars lit by latency band; colour carries the same band for a second cue.
  const bars = $derived(ms < 60 ? 3 : ms < 120 ? 2 : 1);
  const tone = $derived(ms < 60 ? 'ok' : ms < 120 ? 'warn' : 'bad');
</script>

<span class="ping" data-tone={tone}>
  <svg width="12" height="11" viewBox="0 0 12 11" aria-hidden="true">
    {#each [0, 1, 2] as i (i)}
      <rect x={i * 4.5} y={7 - i * 3.5} width="3" height={4 + i * 3.5} rx="1" opacity={i < bars ? 1 : 0.25} />
    {/each}
  </svg>
  {ms} ms
</span>

<style>
  .ping {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ink-2);
  }

  svg {
    fill: var(--ok);
  }

  [data-tone='warn'] svg {
    fill: var(--warn);
  }

  [data-tone='bad'] svg {
    fill: var(--bad);
  }
</style>
