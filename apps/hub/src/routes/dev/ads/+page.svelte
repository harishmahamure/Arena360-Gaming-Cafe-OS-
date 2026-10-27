<script lang="ts">
  import { Button, t } from "@arena/ui";
  import { onMount } from "svelte";
  import { ads, type AdEvent } from "../../../lib/ads";

  /** The most events kept on screen. */
  const KEEP = 50;

  let current = $state(ads.state);
  let events: AdEvent[] = $state([]);
  let box: HTMLDivElement | undefined = $state();

  onMount(() => {
    const stopState = ads.onChange((s) => (current = s));
    const stopEvents = ads.onEvent((e) => (events = [e, ...events].slice(0, KEEP)));
    current = ads.state;
    return () => {
      stopState();
      stopEvents();
      ads.banner.destroy();
    };
  });

  // The dev gallery works only where the plugin runs and was built for debugging.
  const open = $derived(current.enabled && current.debug);

  function placeBanner() {
    const r = box?.getBoundingClientRect();
    if (r) ads.banner.place({ x: r.left, y: r.top, width: r.width, height: r.height });
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.dev_ads.title") })}</title>
</svelte:head>

<section class="gallery" aria-labelledby="dev-ads-title">
  <h1 class="type-hub-title" id="dev-ads-title">{t("hub.dev_ads.title")}</h1>
  {#if !open}
    <p class="type-hub-body" data-testid="dev-ads-unavailable">{t("hub.dev_ads.unavailable")}</p>
  {:else}
    <div class="actions">
      <Button label="hub.dev_ads.banner_place" onclick={placeBanner} />
      <Button label="hub.dev_ads.banner_hide" variant="secondary" onclick={ads.banner.hide} />
      <Button label="hub.dev_ads.interstitial_preload" onclick={ads.interstitial.preload} />
      <Button label="hub.dev_ads.interstitial_show" onclick={ads.interstitial.show} />
    </div>
    <div class="slot" bind:this={box} data-testid="dev-ads-slot"></div>
    <h2 class="type-hub-title">{t("hub.dev_ads.events")}</h2>
    {#if events.length === 0}
      <p class="type-hub-body">{t("hub.dev_ads.no_events")}</p>
    {:else}
      <ol class="events" data-testid="dev-ads-events">
        {#each events as e, i (events.length - i)}
          <li class="type-hub-label">{e.event} · {e.slot}</li>
        {/each}
      </ol>
    {/if}
  {/if}
</section>

<style>
  .gallery {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }

  h1,
  h2,
  p {
    margin: 0;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .slot {
    min-block-size: 3.5rem;
    border: 1px solid var(--line);
    background-color: var(--surface);
  }

  .events {
    display: grid;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }
</style>
