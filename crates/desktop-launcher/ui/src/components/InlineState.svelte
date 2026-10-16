<script lang="ts">
  interface Props {
    message: string;
    onretry?: () => void;
  }

  let { message, onretry }: Props = $props();
</script>

<div class="state" role={onretry ? 'alert' : undefined}>
  <p>{message}</p>
  {#if onretry}<button onclick={onretry}>Try again</button>{/if}
</div>

<style>
  .state {
    display: flex;
    align-items: center;
    gap: var(--s-4);
    padding: var(--s-4) 0;
    color: var(--ink-2);
    font: var(--t-meta);
    font-size: 13px;
  }

  button {
    padding: 6px var(--s-3);
    border-radius: var(--r-sm);
    background: var(--raise-2);
    font: var(--t-label);
    white-space: nowrap;
    transition: background-color var(--t-fast) linear;
  }

  button:hover {
    background: rgb(255 255 255 / 0.16);
  }
</style>
