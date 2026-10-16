<script lang="ts">
  import { formatCount } from '../lib/format';

  let { online }: { online?: number } = $props();
</script>

{#if online !== undefined}
  <span class="online"><span class="dot" aria-hidden="true"></span>{formatCount(online)} online</span>
{/if}

<style>
  .online {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ink-2);
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 1px;
    background: var(--ok);
  }
</style>
