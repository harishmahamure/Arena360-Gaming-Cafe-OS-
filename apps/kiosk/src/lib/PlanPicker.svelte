<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type {
    AttractView,
    KioskHubState,
    PlanOption,
    PlayerContext,
    PlayerPlansResult,
  } from "@arena/ui/generated";
  import { onMount, tick, untrack } from "svelte";
  import { ruleChipText } from "./attract";
  import { playerPlans, playerStart } from "./bridge";
  import {
    START_WAIT_MS,
    detailLines,
    kindLabel,
    nextSelectable,
    optionName,
    refusalText,
    tagLabel,
  } from "./plans";

  /**
   * The plan picker (Story 6.9): after a login, the options valid for this PC (owned packs and
   * passes, block plans, then the wallet) as cards. One is chosen, "Start" asks the Hub to begin
   * it, and "Waiting for the counter…" shows until the session screen replaces this one. A
   * refusal keeps the player here with the reason. The Hub knows who logged in; this screen never
   * names a player.
   */
  let {
    view,
    hub,
    player,
    onlogout,
  }: {
    view: AttractView | null;
    /** The Hub link as the service reports it (`null` until it says). */
    hub: KioskHubState | null;
    player: PlayerContext;
    /** Forget the player and go back to attract ("Not you?", or the Hub lost the login). */
    onlogout: () => void;
  } = $props();

  let result = $state<PlayerPlansResult | null>(null);
  let loaded = $state(false);
  /** Index into the options of the chosen card. */
  let selected: number | null = $state(null);
  /** Index of the card that takes Tab (and the arrow keys' starting point). */
  let active = $state(0);
  /** A start is out, or accepted and waiting for the session screen. */
  let waiting = $state(false);
  let waitingName = $state("");
  /** The line under the cards: a refusal's reason, or the counter being unreachable. */
  let problem: string | null = $state(null);
  let cards: Array<HTMLButtonElement | undefined> = $state([]);
  let waitTimer: ReturnType<typeof setTimeout> | undefined;
  let gone = false;

  const options = $derived(result?.options ?? []);
  const locale = $derived(view?.locale ?? null);
  const connected = $derived(hub === "connected");
  const chosen = $derived(selected === null ? null : (options[selected] ?? null));
  const canStart = $derived(chosen !== null && chosen.selectable && connected && !waiting);

  /** Reads the options; a lost login goes back to attract, an unreachable Hub says so. */
  async function refresh(focus = false) {
    const outcome = await playerPlans();
    if (gone) return;
    if (outcome.kind === "login") return onlogout();
    if (outcome.kind === "unreachable") {
      loaded = true;
      problem ??= t("player.hub.reconnecting_retry");
      return;
    }
    result = outcome.result;
    loaded = true;
    // A choice that is no longer on offer is dropped.
    if (selected !== null && !outcome.result.options[selected]?.selectable) selected = null;
    const first = outcome.result.options.findIndex((o) => o.selectable);
    if (!outcome.result.options[active]?.selectable) active = Math.max(first, 0);
    if (focus) {
      await tick();
      cards[active]?.focus();
    }
  }

  onMount(() => {
    void refresh(true);
    return () => {
      gone = true;
      clearTimeout(waitTimer);
    };
  });

  // The link coming back clears the "reconnecting" line and reads the options if there are none.
  $effect(() => {
    const up = connected;
    untrack(() => {
      if (up && loaded && !result && !waiting) {
        problem = null;
        void refresh(true);
      }
    });
  });

  function choose(i: number) {
    if (waiting || !options[i]?.selectable) return;
    selected = i;
    active = i;
    problem = null;
  }

  function onKeyDown(e: KeyboardEvent) {
    if (waiting) return;
    const step =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (step === 0) return;
    e.preventDefault();
    const at = nextSelectable(options, active, step);
    if (at < 0) return;
    active = at;
    cards[at]?.focus();
  }

  async function start() {
    const option = chosen;
    if (!option || !canStart) return;
    problem = null;
    waiting = true;
    waitingName = optionName(option);
    const outcome = await playerStart(option.choice);
    if (gone) return;
    switch (outcome.kind) {
      case "ok":
        // The session screen replaces this one; if it does not come, say so and let them retry.
        waitTimer = setTimeout(() => {
          waiting = false;
          problem = t("player.hub.reconnecting_retry");
        }, START_WAIT_MS);
        return;
      case "login":
        return onlogout();
      case "refused":
        waiting = false;
        selected = null;
        problem = locale
          ? refusalText(outcome.code, result?.wallet, locale)
          : t("player.picker.refused.other");
        await refresh(true);
        return;
      default:
        waiting = false;
        problem = t("player.hub.reconnecting_retry");
    }
  }

  const rule = $derived(result?.wallet?.rule ?? null);
</script>

<section class="picker" data-testid="plan-picker" aria-busy={!loaded || waiting}>
  <h2 class="type-kiosk-title" id="login-title">{t("player.picker.title")}</h2>
  <p class="who type-kiosk-body" data-testid="picker-player">
    {t("player.login.holding.title", { name: player.name })}
  </p>

  {#if !loaded}
    <p class="type-kiosk-body" data-testid="picker-loading" role="status">
      {t("player.picker.loading")}
    </p>
  {:else if options.length === 0}
    <p class="type-kiosk-body" data-testid="picker-none">{t("player.picker.none")}</p>
  {:else}
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div
      class="grid"
      role="radiogroup"
      aria-label={t("player.picker.options_label")}
      data-testid="picker-options"
      onkeydown={onKeyDown}
    >
      {#each options as option, i (i)}
        {@const tag = tagLabel(option)}
        <button
          type="button"
          role="radio"
          class="card"
          class:selected={selected === i}
          class:muted={!option.selectable}
          data-testid="picker-option"
          data-mode={option.choice.mode}
          aria-checked={selected === i ? "true" : "false"}
          tabindex={active === i && option.selectable ? 0 : -1}
          disabled={!option.selectable || waiting}
          bind:this={cards[i]}
          onclick={() => choose(i)}
        >
          <span class="kind type-kiosk-label">{kindLabel(option)}</span>
          <span class="name type-kiosk-title">{optionName(option)}</span>
          {#if tag}
            <span class="tag type-kiosk-label" data-testid="picker-tag">
              <span aria-hidden="true">⊘</span>
              {tag}
            </span>
          {/if}
          {#if locale}
            {#each detailLines(option, result?.wallet, locale) as line, n (n)}
              <span class="line type-kiosk-body tabular-nums">{line}</span>
            {/each}
          {/if}
          {#if option.choice.mode === "wallet" && rule && locale}
            <span
              class="chip-rule type-kiosk-label"
              data-testid="picker-chip-rule"
              aria-label={t("player.attract.rule_label")}
            >
              <span aria-hidden="true">⚡</span>
              {ruleChipText(rule, locale)}
            </span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}

  <p class="error type-kiosk-label" data-testid="picker-error" role={problem ? "alert" : undefined}>
    {#if problem}<span aria-hidden="true">⚠</span> {problem}{/if}
  </p>

  <div class="actions">
    {#if waiting}
      <div class="waiting" data-testid="picker-waiting">
        <Button
          label="player.picker.start"
          disabled={true}
          disabledReason="player.picker.waiting"
        />
        <p class="type-kiosk-body" data-testid="picker-waiting-plan">{waitingName}</p>
      </div>
    {:else if !connected}
      <Button
        label="player.picker.start"
        disabled={true}
        disabledReason="player.hub.reconnecting_retry"
      />
    {:else if !canStart}
      <Button
        label="player.picker.start"
        disabled={true}
        disabledReason="player.picker.start_disabled"
      />
    {:else}
      <Button label="player.picker.start" onclick={start} />
    {/if}
    <button
      type="button"
      class="logout type-kiosk-label"
      data-testid="login-logout"
      disabled={waiting}
      onclick={onlogout}
    >
      {t("player.login.holding.logout")}
    </button>
  </div>
</section>

<style>
  .picker {
    display: grid;
    gap: var(--space-4);
    justify-items: stretch;
    width: min(60rem, 84vw);
    max-height: 84vh;
    overflow-y: auto;
    text-align: start;
  }

  .picker > * {
    margin: 0;
    overflow-wrap: anywhere;
  }

  h2,
  .who {
    text-align: center;
  }

  .who {
    color: var(--text-muted);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr));
    gap: var(--space-4);
  }

  .card {
    display: grid;
    gap: var(--space-2);
    align-content: start;
    justify-items: start;
    min-height: var(--space-touch-min);
    padding: var(--space-4);
    border: 2px solid var(--card-border, var(--border-strong));
    border-radius: var(--radius-lg);
    background-color: var(--card-background);
    color: var(--text);
    font: inherit;
    text-align: start;
    cursor: pointer;
  }

  .card.selected {
    border: 2px solid var(--text);
  }

  .card:focus-visible {
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .card.muted {
    color: var(--text-muted);
    cursor: not-allowed;
  }

  .card > * {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .kind {
    color: var(--text-muted);
  }

  .tag {
    display: inline-flex;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    font-weight: 600;
  }

  .chip-rule {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-full);
    background-color: var(--chip-rule-background);
    color: var(--chip-rule-foreground);
  }

  .error {
    min-height: 1.5em;
    color: var(--text-field-error-text);
    font-weight: 600;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    align-items: flex-start;
    justify-content: center;
  }

  .waiting {
    display: grid;
    gap: var(--space-2);
    justify-items: start;
  }

  .waiting p {
    margin: 0;
    font-weight: 600;
  }

  .logout {
    min-width: var(--space-touch-min);
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-5);
    border: 2px solid var(--border-strong);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }

  .logout:disabled {
    color: var(--text-muted);
    cursor: not-allowed;
  }

  .logout:focus-visible {
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }
</style>
