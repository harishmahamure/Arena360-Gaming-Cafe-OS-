<script lang="ts">
  import "@arena/ui/base.css";
  import { t } from "@arena/ui";
  import type { KioskHubState } from "@arena/ui/generated";
  import { afterNavigate, goto } from "$app/navigation";
  import { page } from "$app/state";
  import { onMount, type Snippet } from "svelte";
  import {
    currentHubStatus,
    onHubStatus,
    onRestoreState,
    reportAppState,
    restoredRoute,
    startHeartbeat,
    takeRestoreState,
  } from "../lib/bridge";
  import { installKioskGuards, placeOnPrimary } from "../lib/guards";

  let { children }: { children: Snippet } = $props();

  /** The window-filling stage; positioned over the primary monitor when the shell says where. */
  let stage: HTMLDivElement;

  /** The Hub link as the Kiosk service reports it (unknown until it says). */
  let hub: KioskHubState | null = $state(null);

  /** Where the player is: the path and query. */
  const here = () => page.url.pathname + page.url.search;

  function restore(state: unknown) {
    const route = restoredRoute(state);
    if (route && route !== here()) void goto(route, { replaceState: true });
  }

  onMount(() => {
    const stops: Array<() => void> = [
      startHeartbeat(),
      installKioskGuards(),
      placeOnPrimary(stage),
    ];
    let gone = false;
    void (async () => {
      const add = (stop: () => void) => (gone ? stop() : stops.push(stop));
      add(await onHubStatus((s) => (hub = s.state)));
      add(
        await onRestoreState((state) => {
          restore(state);
          // Delivered: clear the shell's copy so a reload does not restore it again.
          void takeRestoreState();
        }),
      );
      const known = await currentHubStatus();
      if (known && hub === null) hub = known.state;
      const pending = await takeRestoreState();
      if (pending) restore(pending);
    })();
    return () => {
      gone = true;
      for (const stop of stops) stop();
    };
  });

  afterNavigate(() => reportAppState({ route: here() }));
</script>

<div class="kiosk" bind:this={stage}>
  {@render children()}
</div>

<!-- Non-blocking: a quiet pill while the Hub is unreachable, announced politely. -->
<div class="link" role="status" aria-live="polite" data-testid="link-status">
  {#if hub === "reconnecting"}
    <span class="pill type-kiosk-label">{t("player.hub.reconnecting")}</span>
  {:else if hub === "token_rejected"}
    <span class="pill type-kiosk-label">{t("player.hub.token_rejected")}</span>
  {/if}
</div>

<style>
  /* The kiosk window is full screen; the app fills all of it. */
  .kiosk {
    position: fixed;
    inset: 0;
    overflow: auto;
    background-color: var(--bg);
  }

  .link {
    position: fixed;
    top: var(--space-kiosk-margin);
    right: var(--space-kiosk-margin);
    pointer-events: none;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-full);
    background-color: var(--reconnecting-indicator-background);
    color: var(--reconnecting-indicator-foreground);
  }
</style>
