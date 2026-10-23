<script lang="ts">
  import "@arena/ui/base.css";
  import { Button, t } from "@arena/ui";
  import { onMount, tick, untrack, type Snippet } from "svelte";
  import { goto, invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import Clock from "../lib/Clock.svelte";
  import Icon from "../lib/Icon.svelte";
  import MoreSheet from "../lib/MoreSheet.svelte";
  import { BOTTOM_BAR, MORE, SCREENS, screenOf, shortcutTarget, type Screen } from "../lib/nav";
  import { SNAPSHOT_DEPENDENCY } from "../lib/snapshot";
  import { SESSION_DEPENDENCY, getToken, safeRedirect, signOut } from "../lib/auth";
  import { connectEvents } from "../lib/events";
  import { applySettingsChanged, setLocale } from "../lib/locale.svelte";
  import type { LayoutData } from "./$types";

  let { data, children }: { data: LayoutData; children: Snippet } = $props();

  /** Auto-retry delays while the Hub is unreachable, in seconds (then 30 s each). */
  const RETRY_DELAYS = [2, 4, 8, 16, 30];
  /** The width at which the rail replaces the bottom bar. */
  const WIDE = "(min-width: 840px)";
  /** Sent by the Tauri shell once it has started (or found) the Hub service. */
  const HUB_READY = "arena:hub-ready";

  /** The sign-in screen: outside the gate, with no rail. */
  const SIGN_IN = "/sign-in";

  const current = $derived(screenOf(page.url.pathname));
  const onSignIn = $derived(page.url.pathname === SIGN_IN);
  const staff = $derived(data.session.state === "signed_in" ? data.session.staff : null);
  const signedOut = $derived(data.session.state === "signed_out");
  /** The Hub answered the snapshot and said who is signed in (an unknown session retries). */
  const reachable = $derived(data.hub.ok && data.session.state !== "unknown");
  /** Not known to be signed in: only the sign-in screen (or the retry state) is shown. */
  const gated = $derived(onSignIn || !staff);
  const inMore = $derived(MORE.some((s: Screen) => s.id === current?.id));
  const branchName = $derived(
    data.hub.ok ? (data.hub.snapshot.branch?.name ?? t("hub.header.no_branch")) : t("app.name"),
  );

  let moreOpen = $state(false);
  let moreButton: HTMLButtonElement | undefined = $state();
  let mainEl: HTMLElement | undefined = $state();
  let refreshing = $state(false);
  /** Seconds until the next automatic retry, while one is scheduled. */
  let nextRetry: number | null = $state(null);
  let failures = 0;
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  let wasOk = untrack(() => reachable);

  /** Focuses `el`, making it programmatically focusable when it is not. */
  function focusEl(el: HTMLElement | null | undefined) {
    if (!el) return;
    if (el.tabIndex < 0 && !el.hasAttribute("tabindex")) el.setAttribute("tabindex", "-1");
    el.focus();
  }

  function closeMore(reason: "dismiss" | "navigate") {
    moreOpen = false;
    // Dismissing keeps the current screen and puts focus back on More.
    if (reason === "dismiss") queueMicrotask(() => moreButton?.focus());
  }

  function onkeydown(event: KeyboardEvent) {
    if (gated) return;
    const target = shortcutTarget(event, document.activeElement);
    if (!target) return;
    event.preventDefault();
    moreOpen = false;
    if (current?.id !== target.id) goto(target.href);
  }

  /** Re-reads the snapshot (Retry, the auto-retry, focus, and the shell's ready event). */
  async function refresh() {
    if (refreshing) return;
    clearTimeout(retryTimer);
    nextRetry = null;
    refreshing = true;
    try {
      await invalidate(SNAPSHOT_DEPENDENCY);
    } finally {
      refreshing = false;
    }
  }

  function scheduleRetry() {
    clearTimeout(retryTimer);
    const seconds = RETRY_DELAYS[Math.min(failures, RETRY_DELAYS.length - 1)];
    failures++;
    nextRetry = seconds;
    retryTimer = setTimeout(() => void refresh(), seconds * 1000);
  }

  // The gate: nobody signed in goes to sign-in; a signed-in member never stays on it.
  $effect(() => {
    if (!data.hub.ok) return;
    if (signedOut && !onSignIn) {
      // Remember where we were going, so sign-in returns there.
      const back = page.url.pathname + page.url.search;
      void goto(SIGN_IN + "?" + new URLSearchParams({ redirectTo: back }), { replaceState: true });
    } else if (staff && onSignIn) {
      void goto(safeRedirect(page.url.searchParams.get("redirectTo")), { replaceState: true });
    }
  });

  // The branch locale comes from each (re)loaded snapshot, then live from `/ui/events`.
  $effect(() => {
    const locale = data.hub.ok ? (data.hub.snapshot.locale ?? null) : null;
    if (data.hub.ok) untrack(() => setLocale(locale));
  });

  // While someone is signed in, this view listens to the Hub's push stream, keyed on the token
  // (signing in again, even as the same member, reopens it with the new one).
  const streamToken = $derived(staff ? getToken() : null);
  $effect(() => {
    const token = streamToken;
    if (!token) return;
    return untrack(() => {
      return connectEvents(token, {
        onSettingsChanged: (e) => applySettingsChanged(e.locale, e.keys),
        onResync: () => void refresh(),
        // Refused: find out whether the session is still there (sign-in otherwise).
        onClosed: () => void invalidate(SESSION_DEPENDENCY),
      });
    });
  });

  // The More sheet belongs to the signed-in shell.
  $effect(() => {
    if (gated) untrack(() => (moreOpen = false));
  });

  async function onSignOut() {
    await signOut();
    await invalidate(SESSION_DEPENDENCY);
  }

  // Every load result: back off while unreachable; on recovery stop retrying and move focus
  // to the screen's heading.
  $effect(() => {
    // Read `data` itself (not the `reachable` derived): every load result must re-run this,
    // even when it is unreachable again.
    const ok = data.hub.ok && data.session.state !== "unknown";
    untrack(() => {
      if (ok) {
        clearTimeout(retryTimer);
        nextRetry = null;
        failures = 0;
        if (!wasOk) tick().then(() => focusEl(mainEl?.querySelector("h1")));
      } else {
        scheduleRetry();
      }
      wasOk = ok;
    });
  });

  onMount(() => {
    const onVisible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    const onFocus = () => void refresh();
    const onReady = () => void refresh();
    // Back above 840px the bottom bar (and its More button) is hidden: close the sheet.
    const wide = window.matchMedia(WIDE);
    const onWide = (e: MediaQueryListEvent) => {
      if (!e.matches || !moreOpen) return;
      moreOpen = false;
      tick().then(() => focusEl(mainEl));
    };
    window.addEventListener("focus", onFocus);
    window.addEventListener(HUB_READY, onReady);
    document.addEventListener("visibilitychange", onVisible);
    wide.addEventListener("change", onWide);
    return () => {
      clearTimeout(retryTimer);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener(HUB_READY, onReady);
      document.removeEventListener("visibilitychange", onVisible);
      wide.removeEventListener("change", onWide);
    };
  });
</script>

<svelte:window {onkeydown} />

<svelte:head>
  {#if !reachable}
    <title>{t("hub.page_title", { screen: t("hub.error.unreachable.title") })}</title>
  {/if}
</svelte:head>

{#snippet unreachable()}
  <section class="unreachable" data-testid="hub-unreachable">
    <h1 class="type-hub-numeric">{t("hub.error.unreachable.title")}</h1>
    <p class="type-hub-body">{t("hub.error.unreachable.body")}</p>
    <p class="retry-status type-hub-label" role="status" data-testid="hub-retry-status">
      {#if refreshing}
        {t("hub.error.retrying_status")}
      {:else if nextRetry !== null}
        {t("hub.error.next_retry", { seconds: nextRetry })}
      {/if}
    </p>
    <Button
      label="common.retry"
      disabled={refreshing}
      disabledReason="hub.error.retrying"
      onclick={refresh}
    />
  </section>
{/snippet}

{#if gated}
  <div class="gate">
    <header class="hub-header">
      <p class="branch type-hub-title" data-testid="hub-branch">{branchName}</p>
      <Clock />
    </header>
    <main class="hub-main" bind:this={mainEl}>
      {#if !reachable}
        {@render unreachable()}
      {:else if onSignIn}
        {@render children()}
      {/if}
    </main>
  </div>
{:else}
  <div class="shell" inert={moreOpen}>
    <header class="hub-header">
      <p class="branch type-hub-title" data-testid="hub-branch">{branchName}</p>
      <div class="header-end">
        {#if staff}
          <p class="staff type-hub-label" data-testid="hub-staff">
            {t("hub.header.signed_in", { name: staff.display_name, role: staff.role_name })}
          </p>
          <Button label="hub.header.sign_out" variant="secondary" onclick={onSignOut} />
        {/if}
        <Clock />
      </div>
    </header>

    <nav class="hub-rail" aria-label={t("hub.nav.label")}>
      <ul>
        {#each SCREENS as screen (screen.id)}
          <li>
            <a
              class="rail-item ui-interactive"
              href={screen.href}
              aria-current={current?.id === screen.id ? "page" : undefined}
            >
              <Icon name={screen.id} />
              <span class="type-hub-micro">{t(screen.label)}</span>
            </a>
          </li>
        {/each}
      </ul>
    </nav>

    <main class="hub-main" bind:this={mainEl}>
      {#if data.hub.ok}
        {@render children()}
      {:else}
        {@render unreachable()}
      {/if}
    </main>

    <nav class="hub-bottom-bar" aria-label={t("hub.nav.label")}>
      <ul>
        {#each BOTTOM_BAR as screen (screen.id)}
          <li>
            <a
              class="bar-item ui-interactive"
              href={screen.href}
              aria-current={current?.id === screen.id ? "page" : undefined}
            >
              <Icon name={screen.id} />
              <span class="type-hub-micro">{t(screen.short ?? screen.label)}</span>
            </a>
          </li>
        {/each}
        <li>
          <button
            class="bar-item ui-interactive"
            type="button"
            bind:this={moreButton}
            aria-haspopup="dialog"
            aria-expanded={moreOpen}
            aria-controls={moreOpen ? "hub-more-sheet" : undefined}
            data-active={inMore ? "true" : undefined}
            onclick={() => (moreOpen = true)}
          >
            <Icon name="more" />
            <span class="type-hub-micro">{t("hub.nav.more")}</span>
          </button>
        </li>
      </ul>
    </nav>
  </div>
{/if}

{#if moreOpen}
  <MoreSheet id="hub-more-sheet" current={current?.id} onclose={closeMore} />
{/if}

<style>
  :global(body) {
    margin: 0;
    background-color: var(--hub-bg);
  }

  .shell {
    display: grid;
    grid-template-columns: var(--space-hub-rail-width) minmax(0, 1fr);
    grid-template-rows: var(--space-hub-header-height) minmax(0, 1fr);
    grid-template-areas:
      "header header"
      "rail main";
    height: 100dvh;
    background-color: var(--hub-bg);
    color: var(--text);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* ── Header ─────────────────────────────────────────────────────────────────── */
  .hub-header {
    grid-area: header;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-5);
    min-width: 0;
    padding: 0 var(--space-hub-gutter);
    border-bottom: 1px solid var(--hub-line);
    background-color: var(--hub-header-background);
  }

  .header-end {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    min-width: 0;
  }

  .staff {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Signed out: the header and the sign-in screen, no rail. */
  .gate {
    display: grid;
    grid-template-rows: var(--space-hub-header-height) minmax(0, 1fr);
    height: 100dvh;
    background-color: var(--hub-bg);
    color: var(--text);
  }

  .gate .hub-header {
    grid-area: auto;
  }

  .gate .hub-main {
    grid-area: auto;
  }

  .branch {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Rail ───────────────────────────────────────────────────────────────────── */
  .hub-rail {
    --ring-gap: var(--hub-rail-background);
    grid-area: rail;
    overflow-y: auto;
    border-right: 1px solid var(--hub-line);
    background-color: var(--hub-rail-background);
  }

  .hub-rail ul {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-2);
  }

  .rail-item,
  .bar-item {
    --ui-bg: transparent;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    min-width: var(--space-touch-min);
    min-height: var(--space-9);
    padding: var(--space-2) var(--space-1);
    border: 0;
    border-radius: var(--radius-md);
    color: var(--text-muted);
    font: inherit;
    text-align: center;
    text-decoration: none;
    cursor: pointer;
  }

  /* hub-micro labels in sentence case, so whole words fit the 88px rail. */
  .rail-item span,
  .bar-item span {
    max-width: 100%;
    text-transform: none;
    letter-spacing: normal;
    overflow-wrap: break-word;
  }

  /* The current screen: filled, in full ink and a heavier weight (never colour alone). */
  .rail-item[aria-current="page"] {
    --ui-bg: var(--hub-rail-item-active);
    color: var(--text);
  }

  .bar-item:is([aria-current="page"], [data-active="true"]) {
    --ui-bg: var(--hub-bottom-bar-item-active);
    color: var(--text);
  }

  .rail-item[aria-current="page"] span,
  .bar-item:is([aria-current="page"], [data-active="true"]) span {
    font-weight: var(--type-hub-numeric-weight);
  }

  /* ── Main ───────────────────────────────────────────────────────────────────── */
  .hub-main {
    grid-area: main;
    min-width: 0;
    overflow-y: auto;
    padding: var(--space-hub-gutter);
  }

  .unreachable {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 60ch;
  }

  .unreachable h1,
  .unreachable p {
    margin: 0;
  }

  .unreachable p {
    color: var(--text-muted);
  }

  .retry-status {
    min-height: 1lh;
  }

  /* ── Bottom bar (below 840px it replaces the rail) ──────────────────────────── */
  .hub-bottom-bar {
    display: none;
  }

  @media (max-width: 839.98px) {
    .shell {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: var(--space-hub-header-height) minmax(0, 1fr) var(--space-9);
      grid-template-areas:
        "header"
        "main"
        "bar";
    }

    .hub-header {
      padding: 0 var(--space-hub-gutter-phone);
    }

    .hub-main {
      padding: var(--space-hub-gutter-phone);
    }

    .hub-rail {
      display: none;
    }

    .hub-bottom-bar {
      --ring-gap: var(--hub-bottom-bar-background);
      grid-area: bar;
      display: block;
      border-top: 1px solid var(--border-strong);
      background-color: var(--hub-bottom-bar-background);
    }

    .hub-bottom-bar ul {
      display: grid;
      grid-template-columns: repeat(5, minmax(0, 1fr));
      height: 100%;
    }

    .bar-item {
      width: 100%;
      height: 100%;
      min-height: var(--space-9);
      border-radius: 0;
    }
  }
</style>
