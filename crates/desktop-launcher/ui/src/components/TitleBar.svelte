<script lang="ts">
  import Minus from '@lucide/svelte/icons/minus';
  import Square from '@lucide/svelte/icons/square';
  import X from '@lucide/svelte/icons/x';
  import WifiOff from '@lucide/svelte/icons/wifi-off';
  import mark from '../../../../../packaging/icons/cinnabar.svg';
  import Nav from './Nav.svelte';
  import AccountChip from './AccountChip.svelte';
  import { store } from '../lib/store.svelte';
  import { windowAction } from '../lib/window';
  import { PRODUCT_NAME } from '../lib/product';

</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="lead" data-tauri-drag-region>
    <img class="mark" src={mark} alt="" width="20" height="20" />
    <span class="name" data-tauri-drag-region>{PRODUCT_NAME}</span>
    <div class="tabs"><Nav variant="tabs" /></div>
  </div>

  <div class="trail" data-tauri-drag-region>
    {#if store.offline && store.account?.kind !== 'offline'}
      <p class="offline" role="status"><WifiOff size={14} strokeWidth={2} aria-hidden="true" />Offline</p>
    {/if}
    <AccountChip />
    {#if store.platform !== 'macos'}
      <div class="controls">
        <button aria-label="Minimize" onclick={() => windowAction('minimize')}><Minus size={16} strokeWidth={1.5} /></button>
        <button aria-label="Maximize" onclick={() => windowAction('toggleMaximize')}><Square size={12} strokeWidth={1.5} /></button>
        <button class="close" aria-label="Close" onclick={() => windowAction('close')}><X size={16} strokeWidth={1.5} /></button>
      </div>
    {/if}
  </div>
</header>

<style>
  .titlebar {
    position: relative;
    z-index: 10;
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    height: 56px;
    padding: 0 0 0 20px;
  }

  :global([data-platform='macos']) .titlebar {
    /* The traffic lights sit in this inset. */
    padding-left: 88px;
    padding-right: 14px;
  }

  .lead {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 100%;
    min-width: 0;
  }

  .mark {
    display: block;
    border-radius: 5px;
  }

  .name {
    font: 680 15px / 1 var(--font);
    letter-spacing: var(--track-display);
  }

  .tabs {
    height: 100%;
    margin-left: 36px;
  }

  .trail {
    grid-column: 2;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
    height: 100%;
  }

  .offline {
    display: flex;
    align-items: center;
    gap: 6px;
    font: var(--t-meta);
    color: var(--warn);
  }

  .controls {
    display: flex;
    align-self: stretch;
    margin-left: 4px;
  }

  .controls button {
    display: grid;
    place-items: center;
    width: 46px;
    color: var(--ink-2);
    transition: background-color var(--t-fast) linear;
  }

  .controls button:hover {
    background: var(--raise-2);
    color: var(--ink);
  }

  .controls .close:hover {
    background: #c42b1c;
    color: #fff;
  }

  .controls button:focus-visible {
    outline-offset: -2px;
  }
</style>
