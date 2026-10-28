<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "../messages/index.js";
  import {
    claimVideo,
    isActiveVideo,
    isHubMediaUrl,
    mediaStage,
    nextClaimId,
    onVideoClaims,
  } from "../game-media.js";

  let {
    video = null,
    cover = null,
    icon = null,
    gameRunning = false,
    highContrast,
    scrim = "hero",
    class: className = "",
    children,
    ...rest
  }: {
    /** The entry's curated, flash-screened video (a Hub `/media/<sha256>` URL). */
    video?: string | null;
    /** The cached cover. */
    cover?: string | null;
    /** The launcher or executable icon the kiosk extracted locally (unmatched entries). */
    icon?: string | null;
    /** A game is running: the video is unloaded and only the cover shows. */
    gameRunning?: boolean;
    /** Defaults to the theme of the nearest `[data-theme]` ancestor. */
    highContrast?: boolean;
    /** `hero` for the full-width scrim, `tile` for the tile scrim. */
    scrim?: "hero" | "tile";
    class?: string;
    /** Copy drawn over the media, inside the scrim's copy zone. */
    children?: Snippet;
    [key: string]: unknown;
  } = $props();

  const id = nextClaimId();
  let root: HTMLElement | undefined = $state();
  let detectedHc = $state(false);
  let failed = $state<string[]>([]);
  let playing = $state(false);
  let userPaused = $state(false);
  let active = $state(false);
  let el: HTMLVideoElement | null = $state(null);

  const hc = $derived(highContrast ?? detectedHc);
  const stage = $derived(mediaStage({ video, cover, icon, gameRunning, highContrast: hc, failed }));
  const wantsVideo = $derived(stage === "video");
  // Under a playing video the cover (or icon) stays drawn, so nothing flashes while it loads.
  const still = $derived(
    hc
      ? null
      : ([cover, icon].find((u): u is string => isHubMediaUrl(u) && !failed.includes(u)) ?? null),
  );
  const stillIsIcon = $derived(still !== null && still === icon && still !== cover);
  const showVideo = $derived(wantsVideo && active);

  const fail = (url: string | null | undefined) => {
    if (url && !failed.includes(url)) failed = [...failed, url];
  };

  $effect(() => {
    if (!root || highContrast !== undefined) return;
    const themed = root.closest("[data-theme]");
    const read = () => (detectedHc = themed?.getAttribute("data-theme") === "hc");
    read();
    if (!themed) return;
    const observer = new MutationObserver(read);
    observer.observe(themed, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  });

  $effect(() => {
    if (!wantsVideo) {
      active = false;
      return;
    }
    const release = claimVideo(id);
    const sync = () => (active = isActiveVideo(id));
    const off = onVideoClaims(sync);
    sync();
    return () => {
      off();
      release();
      active = false;
    };
  });

  // A new video URL starts playing again.
  $effect(() => {
    void video;
    playing = false;
    userPaused = false;
  });

  /** Mutes (the attribute alone is not reliable), plays, and frees the file on removal. */
  function player(node: HTMLVideoElement, url: string) {
    node.muted = true;
    node.defaultMuted = true;
    node.volume = 0;
    const start = () => {
      if (!userPaused) node.play().catch(() => fail(url));
    };
    start();
    return {
      destroy() {
        node.pause();
        node.removeAttribute("src");
        node.load();
        playing = false;
      },
    };
  }

  function toggle(node: HTMLVideoElement | null) {
    if (!node) return;
    userPaused = !node.paused;
    if (userPaused) node.pause();
    else node.play().catch(() => fail(video));
  }
</script>

<div
  bind:this={root}
  class="game-media {className}"
  class:hc
  data-component="game-media"
  data-stage={stage}
  data-playing={showVideo && playing ? "true" : "false"}
  {...rest}
>
  {#if still}
    <img
      class="still"
      class:icon={stillIsIcon}
      src={still}
      alt=""
      draggable="false"
      onerror={() => fail(still)}
    />
  {/if}
  {#if showVideo && video}
    <video
      bind:this={el}
      class="video"
      class:ready={playing}
      src={video}
      loop
      muted
      playsinline
      preload="auto"
      disablepictureinpicture
      disableremoteplayback
      aria-hidden="true"
      tabindex="-1"
      use:player={video}
      onloadeddata={() => (playing = true)}
      onplaying={() => (playing = true)}
      onerror={() => fail(video)}
    ></video>
  {/if}
  {#if !hc}
    <div class="scrim {scrim}" data-testid="game-media-scrim" aria-hidden="true"></div>
  {/if}
  {#if children}
    <div class="copy {scrim}" data-testid="game-media-copy">{@render children()}</div>
  {/if}
  {#if showVideo && playing}
    <button
      type="button"
      class="video-control"
      data-testid="video-control"
      aria-label={userPaused ? t("ui.game_media.play") : t("ui.game_media.pause")}
      onclick={() => toggle(el)}
    >
      <svg
        viewBox="0 0 24 24"
        width="20"
        height="20"
        aria-hidden="true"
        focusable="false"
        fill="currentColor"
      >
        {#if userPaused}<path d="M8 5v14l11-7z" />{:else}<path
            d="M6 5h4v14H6zM14 5h4v14h-4z"
          />{/if}
      </svg>
    </button>
  {/if}
</div>

<style>
  .game-media {
    position: relative;
    overflow: hidden;
    background: var(--bg);
    min-width: 0;
    min-height: 0;
  }

  .still,
  .video,
  .scrim {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .still,
  .video {
    object-fit: cover;
    user-select: none;
  }

  /* An icon is small art, not a backdrop. */
  .still.icon {
    inset: auto;
    top: 50%;
    left: 50%;
    width: 25%;
    height: auto;
    max-height: 50%;
    object-fit: contain;
    transform: translate(-50%, -50%);
  }

  /* The video stays invisible until a frame plays, so the cover underneath never flickers. */
  .video {
    opacity: 0;
  }

  .video.ready {
    opacity: 1;
  }

  .scrim {
    pointer-events: none;
  }

  .scrim.hero {
    background:
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

  .scrim.tile {
    background: linear-gradient(
      0deg,
      color-mix(in srgb, var(--scrim-base) 85%, transparent) 0%,
      color-mix(in srgb, var(--scrim-base) 85%, transparent) 25%,
      transparent 55%
    );
  }

  /* Copy sits in the scrim's dense zone: left 45% × bottom 50% (hero), the bottom quarter (tile). */
  .copy {
    position: absolute;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    box-sizing: border-box;
    color: var(--text);
  }

  .copy.hero {
    left: 0;
    bottom: 0;
    width: 45%;
    height: 50%;
    padding: var(--space-4);
  }

  .copy.tile {
    left: 0;
    right: 0;
    bottom: 0;
    height: 25%;
    padding: var(--space-2) var(--space-3);
  }

  .video-control {
    position: absolute;
    right: var(--space-3);
    top: var(--space-3);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--space-touch-min);
    height: var(--space-touch-min);
    padding: 0;
    border: var(--video-control-border);
    border-radius: var(--radius-full);
    background: var(--video-control-background);
    color: var(--video-control-foreground);
    cursor: pointer;
  }
</style>
