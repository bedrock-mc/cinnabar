<script lang="ts">
  import Hero from '../components/Hero.svelte';
  import WhatsNew from '../components/WhatsNew.svelte';
  import NewsCard from '../components/NewsCard.svelte';
  import ServerList from '../components/ServerList.svelte';
  import { store } from '../lib/store.svelte';

  // News is optional garnish: cards appear only when there is a post to show.
  const news = $derived(store.news.status === 'ready' ? store.news.value.items.slice(0, 2) : []);

</script>

<!-- The world fills the window. The player stands centre stage, launch sits left, and a dock of
     what's new, news and servers runs along the foot. -->
<div class="home">
  <div class="hero"><Hero /></div>
  <div class="dock">
    {#if store.changelog}<div class="slot notes"><WhatsNew log={store.changelog} /></div>{/if}
    {#each news as item, i (item.id)}<div class="slot news" class:second={i === 1}><NewsCard {item} /></div>{/each}
    <div class="slot servers"><ServerList /></div>
  </div>
</div>

<style>
  .home {
    display: grid;
    grid-template-rows: 1fr auto;
    height: 100%;
    padding: 0 32px 24px;
  }

  .hero {
    align-self: end;
    padding: 0 0 28px 8px;
  }

  .dock {
    display: flex;
    gap: 14px;
    height: 196px;
  }

  .slot {
    min-width: 0;
  }

  .notes {
    flex: 0 0 300px;
  }

  .news {
    flex: 0 1 236px;
  }

  .servers {
    flex: 1 1 280px;
    min-width: 260px;
  }

  /* A first-run hint stays card-sized rather than stretching into an empty band. */
  .servers:has(:global(.hint)) {
    flex: 0 1 320px;
    align-self: flex-end;
  }


  /* Small windows keep one news card and a shorter dock. */
  @media (max-width: 1080px) {
    .second {
      display: none;
    }
  }

  @media (max-height: 640px) {
    .dock {
      height: 164px;
    }

    .hero {
      padding-bottom: 18px;
    }
  }

  /* One orchestrated arrival: launch first, then the dock cards a beat apart. */
  @media (prefers-reduced-motion: no-preference) {
    .hero,
    .slot {
      animation: rise 700ms var(--ease-out) both;
    }

    .slot:nth-child(1) {
      animation-delay: 90ms;
    }

    .slot:nth-child(2) {
      animation-delay: 140ms;
    }

    .slot:nth-child(3) {
      animation-delay: 190ms;
    }

    .slot:nth-child(4) {
      animation-delay: 240ms;
    }
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(12px);
    }
  }
</style>
