<script lang="ts">
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import Info from '@lucide/svelte/icons/info';
  import type { Snippet } from 'svelte';

  interface Props {
    tone: 'info' | 'warn' | 'error';
    title: string;
    children: Snippet;
    action?: { label: string; run: () => void };
  }

  let { tone, title, children, action }: Props = $props();
</script>

<div class="notice" data-tone={tone} role={tone === 'error' ? 'alert' : 'status'}>
  <span class="icon">
    {#if tone === 'error'}<CircleAlert size={18} strokeWidth={2} aria-hidden="true" />{:else if tone === 'info'}<Info size={18} strokeWidth={2} aria-hidden="true" />{:else}<TriangleAlert size={18} strokeWidth={2} aria-hidden="true" />{/if}
  </span>
  <div class="copy">
    <p class="title">{title}</p>
    <p class="body">{@render children()}</p>
  </div>
  {#if action}
    <button class="action" onclick={action.run}>{action.label}</button>
  {/if}
</div>

<style>
  .notice {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: start;
    gap: var(--s-3);
    padding: 14px var(--s-4);
    border-radius: var(--r-md);
    background: var(--glass-strong);
    -webkit-backdrop-filter: blur(20px) saturate(1.4);
    backdrop-filter: blur(20px) saturate(1.4);
    box-shadow: inset 0 0 0 1px rgb(242 181 80 / 0.22);
  }

  .notice[data-tone='info'] {
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.14);
  }

  [data-tone='info'] .icon {
    color: var(--ink-2);
  }

  .notice[data-tone='error'] {
    box-shadow: inset 0 0 0 1px rgb(255 122 138 / 0.28);
  }

  .icon {
    padding-top: 1px;
    color: var(--warn);
  }

  [data-tone='error'] .icon {
    color: var(--bad);
  }

  .title {
    font: var(--t-label);
    font-size: 14px;
    color: var(--ink);
  }

  .body {
    margin-top: 2px;
    font: var(--t-meta);
    font-size: 13px;
    color: var(--ink-2);
    text-wrap: pretty;
  }

  .action {
    align-self: center;
    padding: 7px var(--s-3);
    border-radius: var(--r-sm);
    background: var(--raise-2);
    font: var(--t-label);
    white-space: nowrap;
    transition: background-color var(--t-fast) linear;
  }

  .action:hover {
    background: rgb(255 255 255 / 0.16);
  }
</style>
