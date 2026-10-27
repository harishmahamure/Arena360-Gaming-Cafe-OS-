<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "@arena/ui";
  import { ads } from "./ads";

  // The permanent `ad-banner` strip (Story 4.11): drawn only while the ads gate is on, with a fixed
  // height from the layout. The Android plugin puts its anchored adaptive banner over exactly this
  // rect; with no ad (no fill, WAN down, load error) the strip stays and shows only its label.

  let slot: HTMLElement | undefined = $state();

  onMount(() => {
    const el = slot;
    if (!el) return;
    let frame = 0;
    let last = "";
    const place = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const r = el.getBoundingClientRect();
        if (r.width < 1 || r.height < 1) {
          // The strip collapsed: the native banner must not stay over whatever took its place.
          if (last) ads.banner.hide();
          last = "";
          return;
        }
        const rect = { x: r.left, y: r.top, width: r.width, height: r.height };
        const key = JSON.stringify(rect);
        if (key === last) return;
        last = key;
        ads.banner.place(rect);
      });
    };
    const resize = new ResizeObserver(place);
    resize.observe(el);
    window.addEventListener("resize", place);
    window.addEventListener("orientationchange", place);
    // The document itself scrolling moves the strip against the (fixed) native banner.
    window.addEventListener("scroll", place, { passive: true });
    window.visualViewport?.addEventListener("resize", place);
    window.visualViewport?.addEventListener("scroll", place);
    place();
    return () => {
      cancelAnimationFrame(frame);
      resize.disconnect();
      window.removeEventListener("resize", place);
      window.removeEventListener("orientationchange", place);
      window.removeEventListener("scroll", place);
      window.visualViewport?.removeEventListener("resize", place);
      window.visualViewport?.removeEventListener("scroll", place);
      // The strip moved to another layout (or the gate went off, which destroys the banner).
      ads.banner.hide();
    };
  });
</script>

<div class="ad-banner" data-component="ad-banner" data-testid="ad-banner" bind:this={slot}>
  <span class="label type-hub-micro" aria-hidden="true">{t("hub.ads.label")}</span>
</div>

<style>
  .ad-banner {
    box-sizing: border-box;
    display: flex;
    align-items: flex-start;
    justify-content: flex-start;
    width: 100%;
    height: var(--ad-slot-height);
    padding: 0 var(--space-2);
    border: 0;
    border-top: var(--ad-banner-border);
    background-color: var(--ad-banner-background);
    overflow: hidden;
  }

  .label {
    color: var(--text-muted);
    line-height: 1;
    padding-top: var(--space-1);
  }
</style>
