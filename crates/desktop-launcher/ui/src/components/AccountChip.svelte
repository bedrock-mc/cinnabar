<script lang="ts">
  import LogIn from '@lucide/svelte/icons/log-in';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import Users from '@lucide/svelte/icons/users';
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import SkinHead from './SkinHead.svelte';
  import { store } from '../lib/store.svelte';

  let open = $state(false);
  let root: HTMLElement;
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const pop = (node: Element) => fly(node, { y: reduce ? 0 : -6, duration: reduce ? 0 : 200, easing: cubicOut });
  const who = $derived(store.account?.kind === 'signedIn' || store.account?.kind === 'offline' ? store.account : undefined);

  function onWindowClick(e: MouseEvent) {
    if (open && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={(e) => e.key === 'Escape' && (open = false)} />

<div class="account" bind:this={root}>
  {#if store.account?.kind === 'signedOut'}
    <button class="chip signin" onclick={() => void store.signIn()}>
      <LogIn size={15} strokeWidth={2} aria-hidden="true" />
      Sign in
    </button>
  {:else if who}
    <button class="chip" aria-expanded={open} aria-haspopup="dialog" onclick={() => (open = !open)}>
      <SkinHead skinUrl={who.skinUrl} size={26} />
      <span class="name">{who.gamertag}</span>
      {#if who.kind === 'offline'}<span class="status offline">Offline</span>{/if}
      <ChevronDown size={14} strokeWidth={2} aria-hidden="true" />
    </button>
    {#if open}
      <div class="panel" role="dialog" aria-label="Profile" transition:pop>
        <div class="me">
          <SkinHead skinUrl={who.skinUrl} size={44} />
          <div>
            <p class="me-name">{who.gamertag}</p>
            <p class="me-state" class:offline={who.kind === 'offline'}>{who.kind === 'offline' ? 'Offline, using your saved sign-in' : 'Signed in'}</p>
          </div>
        </div>
        <!-- Friends need a Cinnabar account; the slot says so plainly instead of faking a list. -->
        <div class="friends">
          <p class="friends-title"><Users size={14} strokeWidth={2} aria-hidden="true" />Friends</p>
          <div class="slots" aria-hidden="true">{#each { length: 5 } as _, i (i)}<span></span>{/each}</div>
          <p class="friends-copy">Friends, presence and invites arrive with Cinnabar accounts.</p>
        </div>
      </div>
    {/if}
  {/if}
</div>

<style>
  .account {
    position: relative;
  }

  .chip {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 36px;
    padding: 0 12px 0 5px;
    border-radius: 999px;
    background: var(--glass);
    -webkit-backdrop-filter: blur(16px) saturate(1.4);
    backdrop-filter: blur(16px) saturate(1.4);
    box-shadow: inset 0 0 0 1px var(--glass-edge);
    font: var(--t-label);
    white-space: nowrap;
    transition: background-color var(--t-fast) linear;
  }

  .chip:hover,
  .chip[aria-expanded='true'] {
    background: var(--glass-strong);
  }

  .chip :global(svg) {
    color: var(--ink-3);
  }

  .signin {
    padding: 0 14px;
    gap: 8px;
  }

  .status {
    font: var(--t-meta);
    font-size: 11px;
  }

  .offline {
    color: var(--warn);
  }

  .panel {
    position: absolute;
    z-index: 20;
    top: calc(100% + 8px);
    right: 0;
    width: 290px;
    padding: 16px;
    border-radius: var(--r-lg);
    background: rgb(16 18 30 / 0.92);
    -webkit-backdrop-filter: blur(28px) saturate(1.4);
    backdrop-filter: blur(28px) saturate(1.4);
    box-shadow:
      inset 0 0 0 1px var(--glass-edge),
      0 24px 48px -16px rgb(0 0 0 / 0.6);
  }

  .me {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .me :global(canvas) {
    border-radius: 10px;
  }

  .me-name {
    font: var(--t-title);
    letter-spacing: var(--track-title);
  }

  .me-state {
    font: var(--t-meta);
    color: var(--ok);
  }

  .me-state.offline {
    color: var(--warn);
  }

  .friends {
    margin-top: 16px;
    padding-top: 14px;
    box-shadow: inset 0 1px 0 var(--line);
  }

  .friends-title {
    display: flex;
    align-items: center;
    gap: 7px;
    font: var(--t-label);
  }

  .friends-title :global(svg) {
    color: var(--ink-3);
  }

  .slots {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }

  .slots span {
    width: 32px;
    height: 32px;
    border-radius: 9px;
    box-shadow: inset 0 0 0 1.5px rgb(255 255 255 / 0.1);
    background: rgb(255 255 255 / 0.03);
  }

  .friends-copy {
    margin-top: 10px;
    font: var(--t-meta);
    color: var(--ink-3);
  }
</style>
