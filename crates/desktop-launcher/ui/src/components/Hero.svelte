<script lang="ts">
  import Check from '@lucide/svelte/icons/check';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
  import { slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import PlayButton from './PlayButton.svelte';
  import Notice from './Notice.svelte';
  import SkinHead from './SkinHead.svelte';
  import ServerArt from './ServerArt.svelte';
  import { store } from '../lib/store.svelte';
  import { CHANNEL_LABEL, handsOff, isDevBuild, playModel, type PlayModel, type Stage } from '../lib/play';
  import { formatAgo } from '../lib/format';
  import { PRODUCT_NAME } from '../lib/product';
  import mark from '../../../../../packaging/icons/cinnabar.svg';

  const install = $derived(store.install.status === 'ready' ? store.install.value : undefined);
  const model: PlayModel = $derived(playModel(install, store.install.status === 'error', store.update, store.game, store.now));
  const account = $derived(store.account?.kind === 'signedIn' || store.account?.kind === 'offline' ? store.account : undefined);
  const last = $derived(store.recent[0]);
  const ready = $derived(model.primary.intent === 'play' && store.game?.kind === 'stopped');
  const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const reveal = (node: Element) => slide(node, { duration: reduce ? 0 : 280, easing: cubicOut });

  const STAGES: { id: Stage; label: string }[] = $derived([
    { id: 'download', label: 'Download' },
    { id: 'verify', label: 'Verify' },
    { id: 'install', label: handsOff(install) ? 'Installer' : 'Install' },
  ]);
  const stageIndex = $derived(STAGES.findIndex((s) => s.id === model.stage));

  function activate() {
    const intent = model.primary.intent;
    if (intent === 'play') void store.play();
    else if (intent === 'update') void store.updateAndPlay();
    else if (intent === 'retry') void store.retry();
  }
</script>

{#snippet version()}
  {#if install}
    <span>{CHANNEL_LABEL[install.installedChannel]} {install.version}</span>
    {#if isDevBuild(install)}
      <span class="tag" title={store.update?.kind === 'unavailable' ? store.update.reason : 'An unpublished build'}>Dev build</span>
    {/if}
  {:else if store.install.status === 'loading'}
    <span class="skeleton" aria-label="Loading version"></span>
  {:else}
    <span>Version unknown</span>
  {/if}
{/snippet}

<div class="hero">
  {#if account}
    <div class="identity">
      <SkinHead skinUrl={account.skinUrl} size={44} />
      <div>
        <p class="name">{account.gamertag}</p>
        <p class="version">{@render version()}</p>
      </div>
    </div>
  {:else}
    <div class="identity">
      <img class="mark" src={mark} alt="" width="44" height="44" />
      <div>
        <p class="name">{PRODUCT_NAME}</p>
        <p class="version">{@render version()}</p>
      </div>
    </div>
  {/if}

  {#if model.notice}
    {@const notice = model.notice}
    <div class="notice" transition:reveal>
      <Notice tone={notice.tone} title={notice.title} action={notice.action ? { label: notice.action.label, run: () => void store.act(notice.action!.intent) } : undefined}>
        {notice.body}
      </Notice>
    </div>
  {/if}

  <div class="actions">
    <PlayButton primary={model.primary} onactivate={activate} />
    {#if model.secondary}
      <button class="secondary" onclick={() => void store.play()}>{model.secondary.label}</button>
    {:else if last && ready}
      <button class="rejoin" onclick={() => void store.rejoin(last)} title="Join {last.name} directly, last played {formatAgo(last.lastPlayedAt, store.now)}">
        <span class="thumb"><ServerArt name={last.name} imageUrl={last.imageUrl} /></span>
        <span class="copy">
          <span class="what"><RotateCcw size={11} strokeWidth={2.5} aria-hidden="true" />Rejoin</span>
          <span class="where">{last.name}</span>
        </span>
      </button>
    {/if}
  </div>

  <div class="status" aria-live="polite">
    {#if model.stage}
      <ol class="stages" aria-label="Update progress">
        {#each STAGES as stage, i (stage.id)}
          <li class:done={i < stageIndex} class:current={i === stageIndex} aria-current={i === stageIndex ? 'step' : undefined}>
            <span class="tick">{#if i < stageIndex}<Check size={10} strokeWidth={3} aria-hidden="true" />{/if}</span>
            {stage.label}
          </li>
        {/each}
      </ol>
    {:else if model.status}
      {@const status = model.status}
      {#if status.text}
        <span class="line" data-tone={status.tone}>
          {#if status.busy}<LoaderCircle class="spin" size={13} strokeWidth={2} aria-hidden="true" />{:else if status.tone === 'warn'}<span class="dot" aria-hidden="true"></span>{/if}
          {status.text}
        </span>
      {/if}
      {#if status.action}
        <button class="link" onclick={() => void store.act(status.action!.intent)}>{status.action.label}</button>
      {/if}
    {/if}
  </div>
</div>

<style>
  .hero {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
  }

  .identity {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .identity :global(canvas) {
    border-radius: 10px;
    box-shadow: 0 6px 20px -6px rgb(0 0 0 / 0.6);
  }

  .name {
    font: var(--t-hero);
    letter-spacing: var(--track-display);
  }

  .version {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
    font: var(--t-label);
    color: var(--ink-2);
  }

  .mark {
    display: block;
    filter: drop-shadow(0 6px 14px rgb(0 0 0 / 0.45));
  }

  .tag {
    padding: 2px 7px;
    border-radius: 6px;
    background: var(--raise-2);
    font: var(--t-meta);
    font-size: 11px;
    color: var(--ink-2);
    letter-spacing: 0.01em;
  }

  .skeleton {
    width: 140px;
    height: 11px;
    border-radius: 4px;
    background: var(--raise-2);
  }

  .notice {
    width: 100%;
    max-width: 420px;
    margin-top: 18px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 20px;
  }

  .secondary {
    padding: 8px 6px;
    font: var(--t-label);
    font-size: 14px;
    color: var(--ink-2);
    text-decoration: underline;
    text-decoration-color: transparent;
    text-underline-offset: 4px;
    transition: color var(--t-fast) linear, text-decoration-color var(--t-fast) linear;
  }

  .secondary:hover {
    color: var(--ink);
    text-decoration-color: currentColor;
  }

  .rejoin {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 62px;
    padding: 0 20px 0 9px;
    border-radius: 14px;
    background: var(--glass);
    -webkit-backdrop-filter: blur(20px) saturate(1.4);
    backdrop-filter: blur(20px) saturate(1.4);
    box-shadow: inset 0 0 0 1px var(--glass-edge);
    text-align: left;
    transition:
      transform var(--t-med) var(--ease-out),
      background-color var(--t-fast) linear;
  }

  .rejoin:hover {
    background: var(--glass-strong);
    transform: translateY(-1px);
  }

  .rejoin:active {
    transform: translateY(1px) scale(0.99);
  }

  .thumb {
    width: 44px;
    height: 44px;
    border-radius: 10px;
    overflow: hidden;
  }

  .copy {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .what {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font: var(--t-meta);
    color: var(--ink-3);
  }

  .where {
    font: var(--t-label);
    font-size: 14px;
    max-width: 150px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }


  .status {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 20px;
    margin-top: 14px;
    font: var(--t-meta);
    color: var(--ink-3);
  }

  .line {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }

  .line[data-tone='warn'] {
    color: var(--ink-2);
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--warn);
  }

  .link {
    font: var(--t-meta);
    color: var(--ink-2);
    font-weight: 600;
    text-decoration: underline;
    text-decoration-color: rgb(201 194 187 / 0.4);
    text-underline-offset: 3px;
  }

  .link:hover {
    color: var(--ink);
    text-decoration-color: currentColor;
  }

  .stages {
    display: flex;
    gap: 18px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .stages li {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .tick {
    display: grid;
    place-items: center;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1.5px currentColor;
  }

  .done {
    color: var(--ink-2);
  }

  .done .tick {
    background: var(--ink-2);
    color: var(--solid);
    box-shadow: none;
  }

  .current {
    color: var(--ink);
    font-weight: 650;
  }

  .current .tick {
    box-shadow: inset 0 0 0 4px var(--cinnabar);
  }

  :global(.spin) {
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(.spin) {
      animation: none;
    }
  }
</style>
