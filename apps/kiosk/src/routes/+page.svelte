<script lang="ts">
  import { t } from "@arena/ui";
  import { onMount } from "svelte";
  import {
    currentPcScreen,
    currentTestSession,
    onPcScreen,
    onTestSession,
    onTestSessionEnded,
    type PcScreen,
  } from "../lib/bridge";

  let endsAt: number | null = $state(null);
  let now = $state(performance.now());
  const left = $derived(endsAt === null ? 0 : Math.max(0, Math.ceil((endsAt - now) / 1000)));
  const clock = $derived([Math.floor(left / 60), String(left % 60).padStart(2, "0")].join(":"));

  let session = $state<{ name: string; from: number; warning: number | null } | null>(null);
  const elapsed = $derived(
    session === null ? 0 : Math.max(0, Math.floor((now - session.from) / 1000)),
  );
  const watch = $derived(
    [Math.floor(elapsed / 3600), Math.floor((elapsed % 3600) / 60), elapsed % 60]
      .map((n, i) => (i === 0 ? String(n) : String(n).padStart(2, "0")))
      .join(":"),
  );

  const follow = (s: PcScreen) => {
    now = performance.now();
    session = s.session
      ? {
          name: s.session.device_name,
          from: now - s.session.elapsed_ms,
          warning: s.session.warning_min,
        }
      : null;
  };

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
      add(await onPcScreen(follow));
      const screen = await currentPcScreen();
      if (screen && session === null) follow(screen);
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
{:else if session !== null}
  <main class="locked" data-testid="session">
    <p class="brand type-kiosk-eyebrow">{t("app.name")}</p>
    <h1 class="type-kiosk-headline" data-testid="session-device">{session.name}</h1>
    <p class="clock type-kiosk-display tabular-nums" data-testid="session-clock">{watch}</p>
    {#if session.warning !== null}
      <p class="warning type-kiosk-body" role="status" data-testid="session-warning">
        <span aria-hidden="true">⚠</span>
        {t("player.session.warning", { minutes: session.warning })}
      </p>
    {/if}
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
  .clock,
  .warning {
    margin: 0;
  }

  .warning {
    color: var(--text);
    font-weight: 600;
  }

  .brand,
  .hint {
    color: var(--text-muted);
  }
</style>
