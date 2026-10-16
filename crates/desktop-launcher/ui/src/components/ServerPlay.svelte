<script lang="ts">
  import Play from '@lucide/svelte/icons/play';
  import type { FeaturedServer } from '../api';
  import { store } from '../lib/store.svelte';
  import { joinBlockedReason } from '../lib/play';

  interface Props {
    server: FeaturedServer;
    /** Solid cinnabar for the one server the layout features; glass otherwise. */
    emphasis?: boolean;
  }

  let { server, emphasis = false }: Props = $props();
  const reason = $derived(joinBlockedReason(store.update, store.game));
</script>

<button class="join" class:emphasis disabled={reason !== undefined} title={reason} aria-label={`Play on ${server.name}`} onclick={() => void store.join(server)}>
  <Play size={13} strokeWidth={2.5} fill="currentColor" aria-hidden="true" />
  Play
</button>

<style>
  .join {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    flex: none;
    height: 34px;
    padding: 0 14px;
    border-radius: 999px;
    background: var(--raise-2);
    font: var(--t-label);
    transition:
      background-color var(--t-fast) linear,
      color var(--t-fast) linear,
      transform var(--t-fast) var(--ease-out);
  }

  .join:hover:not(:disabled),
  .emphasis:not(:disabled) {
    background: var(--cinnabar-fill);
    color: var(--cinnabar-ink);
  }

  .join:active:not(:disabled) {
    transform: scale(0.97);
  }

  .join:disabled {
    color: var(--ink-3);
    background: var(--raise);
  }
</style>
