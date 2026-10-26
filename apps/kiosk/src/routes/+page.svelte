<script lang="ts">
  import { t } from "@arena/ui";
  import { onMount } from "svelte";
  import { currentTestSession, onTestSession, onTestSessionEnded } from "../lib/bridge";

  let endsAt: number | null = $state(null);
  let now = $state(performance.now());
  const left = $derived(endsAt === null ? 0 : Math.max(0, Math.ceil((endsAt - now) / 1000)));
  const clock = $derived([Math.floor(left / 60), String(left % 60).padStart(2, "0")].join(":"));

  const start = (remainingMs: number) => {
    now = performance.now();
    endsAt = now + remainingMs;
  };

  onMount(() => {
    const tick = setInterval(() => (now = performance.now()), 250);
    const stops: Array<() => void> = [() => clearInterval(tick)];
    let gone = false;
    void (async () => {
      const add = (stop: () => void) => (gone ? stop() : stops.push(stop));
      add(await onTestSession(start));
      add(await onTestSessionEnded(() => (endsAt = null)));
      const known = await currentTestSession();
      if (known !== null && endsAt === null) start(known);
    })();
    return () => {
      gone = true;
      for (const stop of stops) stop();
    };
  });
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("surface.kiosk") })}</title>
</svelte:head>

{#if endsAt !== null}
  <main class="locked" data-testid="test-session">
    <p class="brand type-kiosk-eyebrow">{t("app.name")}</p>
    <h1 class="type-kiosk-headline">{t("player.test_session.title")}</h1>
    <p class="clock type-kiosk-display tabular-nums" data-testid="test-session-clock">{clock}</p>
    <p class="hint type-kiosk-body">{t("player.test_session.hint")}</p>
  </main>
{:else}
  <!-- No session yet: the locked placeholder (login comes with the session stories). -->
  <main class="locked" data-testid="locked">
    <p class="brand type-kiosk-eyebrow">{t("app.name")}</p>
    <h1 class="type-kiosk-headline">{t("player.locked.ready")}</h1>
    <p class="hint type-kiosk-body">{t("player.locked.hint")}</p>
  </main>
{/if}

<style>
  .locked {
    display: grid;
    place-content: center;
    gap: var(--space-6);
    min-height: 100vh;
    padding: var(--space-kiosk-margin);
    color: var(--text);
    text-align: center;
  }

  h1 {
    max-width: 24ch;
    margin: 0 auto;
  }

  .brand,
  .hint,
  .clock {
    margin: 0;
  }

  .brand,
  .hint {
    color: var(--text-muted);
  }
</style>
