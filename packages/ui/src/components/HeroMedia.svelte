<script lang="ts">
  import { onMount } from "svelte";
  import { shouldPlay } from "../hero-video.js";
  import { t } from "../messages/index.js";

  type VideoSource = { src: string; type: string };

  /** The marketing hero's media: a muted looping video over a still cover, over solid `bg`. */
  let { sources = [] }: { sources?: readonly VideoSource[] } = $props();

  let root: HTMLDivElement | undefined = $state();
  let video: HTMLVideoElement | undefined = $state();
  let reducedMotion = $state(false);
  let visible = $state(false);
  let paused = $state(false);
  let failed = $state(false);
  let highContrast = $state(false);

  const hasVideo = $derived(sources.length > 0 && !failed && !highContrast);
  const playing = $derived(shouldPlay({ reducedMotion, visible, paused, failed: !hasVideo }));

  onMount(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    reducedMotion = query.matches;
    const onChange = (event: MediaQueryListEvent) => (reducedMotion = event.matches);
    query.addEventListener("change", onChange);
    const observer = new IntersectionObserver((entries) => {
      visible = entries.some((entry) => entry.isIntersecting);
    });
    if (root) observer.observe(root);
    // High contrast replaces the media with solid `bg`; the page theme is on <html>.
    const html = document.documentElement;
    const syncTheme = () => (highContrast = html.getAttribute("data-theme") === "hc");
    syncTheme();
    const themes = new MutationObserver(syncTheme);
    themes.observe(html, { attributes: true, attributeFilter: ["data-theme"] });
    return () => {
      query.removeEventListener("change", onChange);
      observer.disconnect();
      themes.disconnect();
    };
  });

  $effect(() => {
    if (!video) return;
    if (playing) video.play().catch(() => (failed = true));
    else video.pause();
  });
</script>

<div class="hero-media" bind:this={root} data-component="hero-media">
  <svg class="cover" viewBox="0 0 1440 900" preserveAspectRatio="xMidYMid slice" aria-hidden="true">
    <rect width="1440" height="900" fill="var(--bg)" />
    <circle cx="1080" cy="260" r="360" fill="var(--surface-2)" />
    <circle cx="1240" cy="640" r="220" fill="var(--surface)" />
    <circle cx="880" cy="520" r="96" fill="none" stroke="var(--accent)" stroke-width="6" />
    <path d="M848 484 936 520 848 556z" fill="var(--text)" opacity="0.9" />
    <rect x="1020" y="120" width="260" height="14" rx="7" fill="var(--state-in-session)" />
    <rect x="1020" y="150" width="180" height="14" rx="7" fill="var(--state-reserved)" />
  </svg>
  {#if hasVideo}
    <video
      bind:this={video}
      class="video"
      muted
      loop
      playsinline
      preload="metadata"
      aria-hidden="true"
      tabindex="-1"
      onerror={() => (failed = true)}
    >
      {#each sources as source, i (source.src)}
        <source
          src={source.src}
          type={source.type}
          onerror={() => {
            if (i === sources.length - 1) failed = true;
          }}
        />
      {/each}
    </video>
  {/if}
  <div class="scrim" aria-hidden="true"></div>
  {#if hasVideo && !reducedMotion}
    <button
      type="button"
      class="ui-interactive video-control"
      aria-pressed={paused}
      aria-label={paused ? t("site.home.video_play") : t("site.home.video_pause")}
      onclick={() => (paused = !paused)}
    >
      <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
        {#if paused}
          <path d="M6 3.5 16 10 6 16.5z" fill="currentColor" />
        {:else}
          <rect x="4.5" y="3.5" width="4" height="13" fill="currentColor" />
          <rect x="11.5" y="3.5" width="4" height="13" fill="currentColor" />
        {/if}
      </svg>
    </button>
  {/if}
</div>

<style>
  .hero-media {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background-color: var(--bg);
  }

  .cover,
  .video,
  .scrim {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .cover,
  .video {
    object-fit: cover;
  }

  /* The scrim: bottom and left gradients of `scrim-base`; hero copy stays inside its copy zone. */
  .scrim {
    background-image:
      linear-gradient(
        0deg,
        color-mix(in srgb, var(--scrim-base) 92%, transparent) 0%,
        color-mix(in srgb, var(--scrim-base) 15%, transparent) 55%,
        color-mix(in srgb, var(--scrim-base) 15%, transparent) 100%
      ),
      linear-gradient(
        90deg,
        color-mix(in srgb, var(--scrim-base) 85%, transparent) 0%,
        color-mix(in srgb, var(--scrim-base) 75%, transparent) 45%,
        transparent 70%
      );
  }

  .video-control {
    --ui-bg: var(--bg);
    position: absolute;
    inset-block-end: var(--space-4);
    inset-inline-end: var(--space-4);
    display: grid;
    place-items: center;
    width: var(--space-touch-min);
    height: var(--space-touch-min);
    padding: 0;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    color: var(--text);
    cursor: pointer;
  }

  /* High contrast: no media, no scrim, solid `bg` only. */
  :global([data-theme="hc"]) .cover,
  :global([data-theme="hc"]) .video,
  :global([data-theme="hc"]) .scrim,
  :global([data-theme="hc"]) .video-control {
    display: none;
  }
</style>
