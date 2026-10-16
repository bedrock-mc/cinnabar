<script lang="ts">
  import { posterFor } from '../lib/poster';

  interface Props {
    name: string;
    imageUrl?: string;
  }

  let { name, imageUrl }: Props = $props();
  let broken = $state(false);
  const poster = $derived(posterFor(name));
</script>

<div class="art" style:--h={poster.hue}>
  {#if imageUrl && !broken}
    <img src={imageUrl} alt="" loading="lazy" decoding="async" onerror={() => (broken = true)} />
  {:else}
    <!-- Typographic poster: a hue field, a fixed-scale block lattice and the name's initial. -->
    <span class="initial" aria-hidden="true">{poster.initial}</span>
  {/if}
</div>

<style>
  .art {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    container-type: size;
    background:
      radial-gradient(120% 90% at 12% 0%, rgb(255 255 255 / 0.22), transparent 60%),
      url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='28' height='48.5'%3E%3Cpath d='M14 0 28 8.08v16.17L14 32.33 0 24.25V8.08zM14 16.17 28 8.08M14 16.17V32.33M14 16.17 0 8.08M14 32.33v16.17M0 24.25v24.25M28 24.25v24.25' fill='none' stroke='white' stroke-opacity='.09' stroke-width='.8'/%3E%3C/svg%3E") 0 0 / 28px 48.5px,
      linear-gradient(160deg, hsl(var(--h) 44% 34%), hsl(var(--h) 46% 20%));
  }

  img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .initial {
    position: absolute;
    right: 6%;
    bottom: -0.18em;
    font: 760 120px / 1 var(--font);
    letter-spacing: -0.04em;
    font-size: min(78cqh, 46cqw);
    color: hsl(var(--h) 62% 64%);
    opacity: 0.5;
    user-select: none;
  }
</style>
