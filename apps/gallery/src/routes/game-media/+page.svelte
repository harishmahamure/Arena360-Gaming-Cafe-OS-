<script lang="ts">
  import { GameMedia, Button, t } from "@arena/ui";

  // Fixture page for the `GameMedia` tests: `?theme=dark|hc&video=a&cover=b&icon=c&count=2`.
  // Each letter names a Hub media URL (`/media/<letter × 64>`); the tests answer those requests.
  const params = new URLSearchParams(window.location.search);
  const theme = params.get("theme") === "hc" ? "hc" : "dark";
  const url = (key: string) => {
    const letter = params.get(key);
    return letter ? `/media/${letter.repeat(64)}` : null;
  };
  const video = url("video");
  const cover = url("cover");
  const icon = url("icon");
  const count = Number(params.get("count") ?? 1);
  const external = params.get("external");

  let running = $state(false);
  let view = $state<"a" | "b">("a");
</script>

<svelte:head>
  <title>{t("gallery.game_media.title")}</title>
</svelte:head>

<main data-theme={theme}>
  <h1>{t("gallery.game_media.heading")}</h1>
  <div class="controls">
    <Button
      variant="secondary"
      label={view === "a" ? "gallery.game_media.view_b" : "gallery.game_media.view_a"}
      onclick={() => (view = view === "a" ? "b" : "a")}
    />
    <Button
      variant="secondary"
      label={running ? "gallery.game_media.game_stopped" : "gallery.game_media.game_running"}
      onclick={() => (running = !running)}
    />
  </div>
  {#key view}
    <div class="views" data-view={view}>
      {#each Array.from({ length: count }, (_, i) => i) as i (i)}
        <GameMedia
          class="frame"
          video={external ?? video}
          {cover}
          {icon}
          gameRunning={running}
          data-testid="media-{i}"
        >
          <h2>{t("gallery.game_media.copy")}</h2>
        </GameMedia>
      {/each}
    </div>
  {/key}
</main>

<style>
  main {
    min-height: 100vh;
    padding: var(--space-4);
    background: var(--bg);
    color: var(--text);
  }

  .controls {
    display: flex;
    gap: var(--space-2);
    margin-block: var(--space-3);
  }

  .views {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
  }

  .views :global(.frame) {
    width: 480px;
    height: 270px;
  }

  h2 {
    margin: 0;
    font-size: var(--type-web-display-size);
  }
</style>
