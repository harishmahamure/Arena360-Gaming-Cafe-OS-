<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "@arena/ui";
  import type { DemoMedia } from "../lib/product";

  /**
   * A product demo: the poster still until the viewer presses play (nothing loads or plays before),
   * then the native player with sound and English captions on. Play, pause, captions and volume
   * are the browser's own labelled controls, and pressing play moves no focus. When the video
   * cannot load or the browser cannot play any source, the poster stays with a plain message.
   */
  let { product, name, media }: { product: string; name: string; media: DemoMedia } = $props();

  let video: HTMLVideoElement | undefined = $state();
  let failed = $state(false);
  let posterFailed = $state(false);

  onMount(() => {
    // A browser that can play none of the sources shows the message at once, not after a click.
    const probe = document.createElement("video");
    if (!media.sources.some((s) => probe.canPlayType(s.type) !== "")) failed = true;
    else if (video?.error) failed = true;
  });
</script>

<figure class="product-media" data-theme="dark" data-failed={failed || undefined}>
  {#if failed}
    <div class="product-still">
      {#if !posterFailed}
        <img src={media.poster} alt="" onerror={() => (posterFailed = true)} />
      {/if}
      <p class="product-unavailable" role="status">{t("site.product.video.unavailable")}</p>
    </div>
  {:else}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video
      bind:this={video}
      controls
      preload="none"
      playsinline
      crossorigin="anonymous"
      poster={media.poster}
      aria-label={t("site.product.video.player", { product: name })}
      onerror={() => (failed = true)}
    >
      {#each media.sources as source, i (source.src)}
        <source
          src={source.src}
          type={source.type}
          onerror={() => {
            if (i === media.sources.length - 1) failed = true;
          }}
        />
      {/each}
      <track
        kind="captions"
        srclang="en"
        label={t("site.product.video.captions")}
        src={media.captions}
        default
      />
      {t("site.product.video.fallback")}
    </video>
  {/if}
  <figcaption>
    <a href={media.transcript} data-product={product}>
      {t("site.product.video.transcript", { product: name })}
    </a>
  </figcaption>
</figure>
