<script lang="ts">
  import { BrandMark, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { callStaff } from "./bridge";
  import { INFO_TOAST_MS } from "./hud";
  import MessagesBell from "./MessagesBell.svelte";
  import VolumeControl from "./VolumeControl.svelte";
  import {
    chipText,
    initialOf,
    isLow,
    playerName,
    type ShellSession,
    TABS,
    type Tab,
  } from "./shell";

  let {
    session,
    logo,
    seconds,
    tab = $bindable("home"),
    status,
  }: {
    session: ShellSession;
    /** The branch logo (a `data:` URI), or `null` for the Arena 360 mark. */
    logo: string | null;
    /** Seconds on the chip: left for prepaid, played for postpaid. */
    seconds: number;
    /** The active item; it lives for the session (the page forgets it when the session ends). */
    tab?: Tab;
    /** The grace or warning line, and the order count: shown under the heading on Home. */
    status?: import("svelte").Snippet;
  } = $props();

  const LABELS: Record<Tab, MessageKey> = {
    home: "player.shell.tab.home",
    games: "player.shell.tab.games",
    apps: "player.shell.tab.apps",
  };

  let brokenLogo: string | null = $state(null);
  let items: HTMLButtonElement[] = $state([]);
  /** The item with the roving focus; Enter or a click makes it the active one. */
  let focused: Tab = $state("home");

  const name = $derived(playerName(session));
  const low = $derived(isLow(session, seconds));
  const time = $derived(chipText(seconds));
  const showLogo = $derived(logo !== null && logo !== brokenLogo);

  /** Call staff was taken by the Hub; the button says so for as long as an info toast stays. */
  let notified = $state(false);
  let calling = false;
  let notifiedTimer: ReturnType<typeof setTimeout> | undefined;

  async function call() {
    if (calling) return;
    calling = true;
    try {
      if (await callStaff()) {
        notified = true;
        clearTimeout(notifiedTimer);
        notifiedTimer = setTimeout(() => (notified = false), INFO_TOAST_MS);
      }
    } finally {
      calling = false;
    }
  }

  function onkeydown(event: KeyboardEvent, index: number) {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const last = TABS.length - 1;
    const next =
      event.key === "ArrowRight"
        ? index === last
          ? 0
          : index + 1
        : event.key === "ArrowLeft"
          ? index === 0
            ? last
            : index - 1
          : -1;
    if (next === -1) return;
    event.preventDefault();
    focused = TABS[next];
    items[next]?.focus();
  }
</script>

<div class="shell" data-testid="shell">
  <header class="bar">
    <div class="logo-slot" data-testid="shell-logo">
      {#if showLogo}
        <img
          src={logo}
          alt={t("player.attract.logo_alt", { name: t("app.name") })}
          draggable="false"
          onerror={() => (brokenLogo = logo)}
        />
      {:else}
        <BrandMark size={48} />
      {/if}
    </div>

    <nav class="pill-nav" aria-label={t("player.shell.nav_label")} data-testid="pill-nav">
      {#each TABS as id, index (id)}
        <button
          bind:this={items[index]}
          type="button"
          class="item type-kiosk-label"
          class:active={tab === id}
          aria-current={tab === id ? "page" : undefined}
          tabindex={focused === id ? 0 : -1}
          data-testid={"tab-" + id}
          onfocus={() => (focused = id)}
          onclick={() => (tab = id)}
          onkeydown={(e) => onkeydown(e, index)}
        >
          {t(LABELS[id])}
        </button>
      {/each}
    </nav>

    <div class="right">
      <VolumeControl />
      <MessagesBell />
      <button
        type="button"
        class="call-staff type-kiosk-label"
        aria-live="polite"
        data-testid="call-staff-button"
        onclick={call}
      >
        <span class="mark" aria-hidden="true">?</span>
        <span data-testid="call-staff-label">
          {t(notified ? "player.call_staff.notified" : "player.call_staff.label")}
        </span>
      </button>
      <p
        class="time-chip"
        class:low
        role="timer"
        aria-label={t(
          low
            ? "player.shell.chip_low"
            : session.prepaid
              ? "player.shell.chip_left"
              : "player.shell.chip_played",
          { time },
        )}
        data-testid="time-chip"
      >
        <span class="dot" aria-hidden="true" data-testid="time-chip-dot">{low ? "⚠" : ""}</span>
        <span class="num type-hud-numeric tabular-nums" data-testid="session-clock">{time}</span>
      </p>
      <p
        class="account type-kiosk-label"
        aria-label={t("player.shell.account", { name })}
        data-testid="account-chip"
      >
        <span class="initial" aria-hidden="true">{initialOf(name)}</span>
        <span class="who" data-testid="account-name">{name}</span>
      </p>
    </div>
  </header>

  <main class="content" aria-label={t(LABELS[tab])}>
    <h1 class="type-kiosk-headline" data-testid="session-device">{session.device}</h1>
    {@render status?.()}
    <p class="empty type-kiosk-body" data-testid={"tab-empty-" + tab}>{t("player.shell.empty")}</p>
  </main>
</div>

<style>
  .shell {
    display: grid;
    grid-template-rows: auto 1fr;
    min-height: 100vh;
    padding: 32px var(--space-kiosk-margin) var(--space-kiosk-margin);
    color: var(--text);
  }

  .bar {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: var(--space-5);
    height: 64px;
  }

  .logo-slot {
    display: flex;
    align-items: center;
    max-width: 30vw;
    height: 64px;
  }

  .logo-slot img {
    display: block;
    max-width: 100%;
    max-height: 64px;
    object-fit: contain;
    user-select: none;
  }

  .pill-nav {
    display: inline-flex;
    gap: var(--space-1);
    padding: var(--space-1);
    border: var(--pill-nav-border);
    border-radius: var(--radius-full);
    background-color: var(--pill-nav-background);
  }

  .item {
    min-width: 96px;
    min-height: 48px;
    padding: 0 var(--space-5);
    border: 0;
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .item.active {
    background-color: var(--pill-nav-item-active-background);
    color: var(--pill-nav-item-active-foreground);
  }

  .item:focus-visible,
  .item:focus {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-4);
    min-width: 0;
  }

  .right > p {
    margin: 0;
  }

  .call-staff {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: var(--call-staff-button-border);
    border-radius: var(--radius-full);
    background-color: var(--call-staff-button-background);
    color: var(--call-staff-button-foreground);
    cursor: pointer;
    white-space: nowrap;
  }

  .call-staff:focus-visible,
  .call-staff:focus {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .mark {
    display: inline-grid;
    place-content: center;
    width: 24px;
    height: 24px;
    border: 2px solid currentColor;
    border-radius: var(--radius-full);
    line-height: 1;
  }

  .time-chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 48px;
    padding: 0 var(--space-4);
    border: 2px solid transparent;
    border-radius: var(--radius-full);
    background-color: var(--time-chip-background);
    color: var(--time-chip-foreground);
  }

  /* The status dot: a solid disc, replaced by a ⚠ glyph (and a 2px warning border) when low. */
  .dot {
    display: inline-grid;
    place-content: center;
    width: 12px;
    height: 12px;
    border-radius: var(--radius-full);
    background-color: var(--time-chip-indicator);
    line-height: 1;
  }

  .time-chip.low {
    border-color: var(--time-chip-indicator-low);
  }

  .time-chip.low .dot {
    width: auto;
    height: auto;
    background: none;
    color: var(--time-chip-indicator-low);
  }

  .account {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }

  .initial {
    display: inline-grid;
    place-content: center;
    width: 40px;
    height: 40px;
    border-radius: var(--radius-full);
    background-color: var(--surface);
    border: 1px solid var(--line);
  }

  .who {
    max-width: 14ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .content {
    display: grid;
    align-content: start;
    justify-items: center;
    gap: var(--space-5);
    padding-top: var(--space-8);
    text-align: center;
  }

  .content > :global(*) {
    margin: 0;
  }

  .empty {
    color: var(--text-muted);
  }
</style>
