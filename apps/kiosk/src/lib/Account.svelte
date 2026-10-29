<script lang="ts">
  import { Button, formatDateTime, t } from "@arena/ui";
  import type { PlayerAccountPage, SnapshotLocale } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, tick, untrack } from "svelte";
  import {
    PASSWORD_MAX_CHARS,
    passwordLeftS,
    recordPasswordChange,
    validNewPassword,
  } from "./account";
  import { balanceText, orderItemsText, orderStatusText, packLines } from "./account-text";
  import { playerAccountPage, playerPasswordChange } from "./bridge";
  import { countdownText } from "./login";

  // The Account page (Story 6.34): `account-subnav` on the left (Details · Wallet & packs · Order
  // history · Change password) and the section on the right. Everything shown is what the Hub
  // said for the player on this PC's session (the phone arrives already masked); the kiosk keeps
  // nothing. Arrow keys move between the sub-nav items. The Change password form sends both
  // passwords to the Kiosk service only and clears them from the fields the moment they are sent.

  let {
    section = $bindable("details"),
    locale,
    connected,
  }: {
    section?: Section;
    /** The branch's money format, once the attract view has told it. */
    locale: SnapshotLocale | null;
    /** The Hub link is up: Hub-dependent actions say "Counter is reconnecting" when it is not. */
    connected: boolean;
  } = $props();

  type Section = "details" | "wallet" | "orders" | "password";
  const SECTIONS: Section[] = ["details", "wallet", "orders", "password"];
  const LABELS: Record<Section, MessageKey> = {
    details: "player.account.section.details",
    wallet: "player.account.section.wallet",
    orders: "player.account.section.orders",
    password: "player.account.section.password",
  };

  const id = $props.id();
  let items: HTMLButtonElement[] = $state([]);
  /** What the Hub said: `null` while loading, `false` when it did not answer. */
  let page: PlayerAccountPage | false | null = $state(null);

  onMount(() => {
    void load();
  });

  async function load() {
    if (!connected) {
      page = false;
      return;
    }
    const result = await playerAccountPage();
    page = result.kind === "ok" ? result.page : false;
  }

  function onkeydown(event: KeyboardEvent, index: number) {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const last = SECTIONS.length - 1;
    const next =
      event.key === "ArrowDown"
        ? index === last
          ? 0
          : index + 1
        : event.key === "ArrowUp"
          ? index === 0
            ? last
            : index - 1
          : -1;
    if (next === -1) return;
    event.preventDefault();
    section = SECTIONS[next];
    items[next]?.focus();
  }

  const date = (ms: number) => formatDateTime(ms, locale?.tz, { dateStyle: "medium" });

  // --- Change password ---
  let current = $state("");
  let fresh = $state("");
  let showCurrent = $state(false);
  let showFresh = $state(false);
  let checking = $state(false);
  let problem: "wrong" | "short" | "failed" | "unreachable" | null = $state(null);
  let saved = $state(false);
  let left = $state(passwordLeftS(performance.now()));
  let currentInput: HTMLInputElement | undefined = $state();
  let freshInput: HTMLInputElement | undefined = $state();
  let gone = false;

  const paused = $derived(left > 0);
  const message = $derived(
    problem === "wrong"
      ? t("player.account.password.wrong")
      : problem === "short"
        ? t("player.account.password.too_short")
        : problem === "failed"
          ? t("player.account.password.failed")
          : problem === "unreachable"
            ? t("player.hub.reconnecting_retry")
            : "",
  );

  onMount(() => () => (gone = true));

  $effect(() => {
    if (!paused) return;
    const timer = setInterval(() => (left = passwordLeftS(performance.now())), 250);
    return () => clearInterval(timer);
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (checking || paused) return;
    saved = false;
    if (!connected) {
      problem = "unreachable";
      return;
    }
    if (!current) return currentInput?.focus();
    if (!validNewPassword(fresh)) {
      problem = "short";
      return freshInput?.focus();
    }
    const [old, next] = [current, fresh];
    // Sent, then gone: the fields never keep them, shown or not.
    current = "";
    fresh = "";
    showCurrent = false;
    showFresh = false;
    problem = null;
    checking = true;
    const outcome = await playerPasswordChange(old, next);
    checking = false;
    recordPasswordChange(outcome, performance.now());
    left = passwordLeftS(performance.now());
    if (gone) return;
    switch (outcome.kind) {
      case "ok":
        saved = true;
        break;
      case "wrong_current":
        problem = "wrong";
        break;
      case "invalid_new":
        problem = "short";
        break;
      case "refused":
        problem = "failed";
        break;
      case "unreachable":
        problem = "unreachable";
        break;
      default:
        // Locked: the button carries the countdown.
        break;
    }
    await tick();
    (problem === "short" ? freshInput : currentInput)?.focus();
  }
</script>

<div
  class="account"
  role="region"
  aria-label={t("player.account.page_label")}
  data-testid="account-page"
>
  <nav class="subnav" aria-label={t("player.account.nav_label")} data-testid="account-subnav">
    {#each SECTIONS as s, index (s)}
      <button
        bind:this={items[index]}
        type="button"
        class="item type-kiosk-label"
        class:active={section === s}
        aria-current={section === s ? "page" : undefined}
        tabindex={section === s ? 0 : -1}
        data-testid={"account-nav-" + s}
        onclick={() => (section = s)}
        onkeydown={(e) => onkeydown(e, index)}
      >
        {t(LABELS[s])}
      </button>
    {/each}
  </nav>

  <section class="content" aria-labelledby="{id}-title" data-testid={"account-section-" + section}>
    <h2 class="type-kiosk-title" id="{id}-title">{t(LABELS[section])}</h2>

    {#if section === "password"}
      <form class="form" data-testid="account-password-form" novalidate onsubmit={submit}>
        <div class="field">
          <label class="type-kiosk-label" for="{id}-current">
            {t("player.account.password.current")}
          </label>
          <div class="row">
            <input
              bind:this={currentInput}
              bind:value={current}
              class="input type-kiosk-body"
              class:invalid={problem === "wrong"}
              id="{id}-current"
              data-testid="account-password-current"
              type={showCurrent ? "text" : "password"}
              name="current"
              autocomplete="off"
              autocapitalize="off"
              spellcheck="false"
              maxlength={PASSWORD_MAX_CHARS}
              disabled={paused}
              aria-invalid={problem === "wrong" ? "true" : undefined}
              aria-describedby={problem === "wrong" ? id + "-error" : undefined}
            />
            <button
              type="button"
              class="toggle type-kiosk-label"
              data-testid="account-password-current-show"
              aria-controls="{id}-current"
              aria-pressed={showCurrent ? "true" : "false"}
              disabled={paused}
              onclick={() => (showCurrent = !showCurrent)}
            >
              {t(showCurrent ? "ui.password.hide" : "ui.password.show")}
            </button>
          </div>
        </div>
        <div class="field">
          <label class="type-kiosk-label" for="{id}-new">{t("player.account.password.new")}</label>
          <div class="row">
            <input
              bind:this={freshInput}
              bind:value={fresh}
              class="input type-kiosk-body"
              class:invalid={problem === "short"}
              id="{id}-new"
              data-testid="account-password-new"
              type={showFresh ? "text" : "password"}
              name="new"
              autocomplete="off"
              autocapitalize="off"
              spellcheck="false"
              maxlength={PASSWORD_MAX_CHARS}
              disabled={paused}
              aria-invalid={problem === "short" ? "true" : undefined}
              aria-describedby={problem === "short" ? id + "-hint " + id + "-error" : id + "-hint"}
            />
            <button
              type="button"
              class="toggle type-kiosk-label"
              data-testid="account-password-new-show"
              aria-controls="{id}-new"
              aria-pressed={showFresh ? "true" : "false"}
              disabled={paused}
              onclick={() => (showFresh = !showFresh)}
            >
              {t(showFresh ? "ui.password.hide" : "ui.password.show")}
            </button>
          </div>
          <p class="hint type-kiosk-label" id="{id}-hint">{t("player.account.password.hint")}</p>
        </div>
        <p
          class="error type-kiosk-label"
          id="{id}-error"
          data-testid="account-password-error"
          role={message ? "alert" : undefined}
        >
          {#if message}<span aria-hidden="true">⚠</span> {message}{/if}
        </p>
        <p
          class="saved type-kiosk-label"
          data-testid="account-password-saved"
          role={saved ? "status" : undefined}
        >
          {#if saved}<span aria-hidden="true">✓</span> {t("player.account.password.saved")}{/if}
        </p>
        <div class="actions">
          {#if paused}
            <div class="tabular-nums" data-testid="account-password-paused">
              <Button
                type="submit"
                label="player.account.password.submit"
                disabled={true}
                disabledReason="player.account.password.locked"
                reasonParams={{ countdown: countdownText(untrack(() => left)) }}
              />
            </div>
          {:else if !connected}
            <Button
              type="submit"
              label="player.account.password.submit"
              disabled={true}
              disabledReason="player.hub.reconnecting_retry"
            />
          {:else}
            <Button
              type="submit"
              label="player.account.password.submit"
              disabled={checking}
              disabledReason="player.unlock.checking"
            />
          {/if}
        </div>
      </form>
    {:else if page === null}
      <p class="type-kiosk-body" role="status" data-testid="account-loading">
        {t("player.account.loading")}
      </p>
    {:else if page === false}
      <p class="type-kiosk-body" role="alert" data-testid="account-error">
        {t("player.hub.reconnecting_retry")}
      </p>
    {:else if section === "details"}
      <dl class="details" data-testid="account-details">
        <div>
          <dt class="type-kiosk-label">{t("player.account.details.name")}</dt>
          <dd class="type-kiosk-body" data-testid="account-detail-name">{page.name}</dd>
        </div>
        <div>
          <dt class="type-kiosk-label">{t("player.account.details.username")}</dt>
          <dd class="type-kiosk-body" data-testid="account-detail-username">{page.username}</dd>
        </div>
        <div>
          <dt class="type-kiosk-label">{t("player.account.details.phone")}</dt>
          <dd class="type-kiosk-body tabular-nums" data-testid="account-detail-phone">
            {page.phone_masked}
          </dd>
        </div>
      </dl>
    {:else if section === "wallet"}
      {#if locale}
        <p class="type-kiosk-headline tabular-nums" data-testid="account-wallet">
          {t("player.account.balance_wallet", { balance: balanceText(page.balance_minor, locale) })}
        </p>
      {/if}
      <p class="type-kiosk-body muted" data-testid="account-wallet-topups">
        {t("player.account.topups")}
      </p>
      {#if page.packs.length === 0}
        <p class="type-kiosk-body" data-testid="account-no-packs">{t("player.account.no_packs")}</p>
      {:else if locale}
        <ul class="packs" data-testid="account-packs">
          {#each page.packs as pack, n (n)}
            <li class="pack" class:expired={pack.expired} data-testid="account-pack">
              <p class="name type-kiosk-label">
                {pack.name}
                {#if pack.expired}
                  <span class="tag type-kiosk-label" data-testid="account-pack-expired">
                    {t("player.account.packs.expired")}
                  </span>
                {/if}
              </p>
              {#each packLines(pack, locale) as line (line)}
                <p class="meta type-kiosk-body tabular-nums">{line}</p>
              {/each}
            </li>
          {/each}
        </ul>
      {/if}
    {:else if page.orders.length === 0}
      <p class="type-kiosk-body" data-testid="account-no-orders">
        {t("player.account.orders.empty")}
      </p>
    {:else if locale}
      <ul class="orders" data-testid="account-orders">
        {#each page.orders as order (order.order_id)}
          <li class="order" data-testid="account-order">
            <p class="name type-kiosk-label">{orderItemsText(order)}</p>
            <p class="meta type-kiosk-body tabular-nums">
              {formatDateTime(order.at, locale.tz)} · {balanceText(order.total_minor, locale)}
            </p>
            <p class="meta type-kiosk-body tabular-nums">
              <span data-testid="account-order-status">{orderStatusText(order.status)}</span> ·
              <span data-testid="account-order-invoice">
                {order.invoice_no
                  ? t("player.account.orders.invoice", { number: order.invoice_no })
                  : t("player.account.orders.no_invoice")}
              </span>
            </p>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<style>
  .account {
    display: grid;
    grid-template-columns: minmax(12rem, 18rem) minmax(0, 1fr);
    gap: var(--space-6);
    align-items: start;
  }

  .subnav {
    display: grid;
    gap: var(--space-1);
    padding: var(--space-3);
    border-radius: var(--radius-xl);
    background-color: var(--account-subnav-background);
  }

  .item {
    display: flex;
    align-items: center;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 0;
    border-left: 3px solid transparent;
    background: transparent;
    color: var(--account-subnav-foreground);
    cursor: pointer;
    text-align: left;
  }

  .item.active {
    border-left-color: var(--account-subnav-active);
    color: var(--account-subnav-active);
  }

  .item:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .content {
    display: grid;
    gap: var(--space-4);
    align-content: start;
    min-width: 0;
    padding: var(--space-5);
    border-radius: var(--radius-xl);
    background-color: var(--surface);
  }

  .content > *,
  .content h2 {
    margin: 0;
  }

  .muted,
  .meta,
  .hint,
  dt {
    color: var(--text-muted);
  }

  .details {
    display: grid;
    gap: var(--space-4);
    margin: 0;
  }

  .details dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .packs,
  .orders {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .pack,
  .order {
    display: grid;
    gap: var(--space-1);
    padding: var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }

  .pack p,
  .order p {
    margin: 0;
  }

  /* An expired pack is muted, with the word as well as the colour. */
  .pack.expired .name,
  .pack.expired .meta {
    color: var(--text-muted);
  }

  .tag {
    margin-inline-start: var(--space-2);
    padding: 2px var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    color: var(--text);
  }

  .form {
    display: grid;
    gap: var(--space-4);
    max-width: 28rem;
  }

  .form > * {
    margin: 0;
  }

  .field {
    display: grid;
    gap: var(--space-2);
    min-width: 0;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .input {
    flex: 1 1 12em;
    min-width: 0;
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--text-field-background);
    color: var(--text-field-foreground);
    font: inherit;
  }

  .input:focus-visible {
    border-width: 2px;
    border-color: var(--accent);
    padding: 0 calc(var(--space-3) - 1px);
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .input.invalid {
    border-width: 2px;
    border-color: var(--danger);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .input:disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .toggle {
    min-width: var(--space-touch-min);
    min-height: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--button-secondary-background);
    color: var(--button-secondary-foreground);
    cursor: pointer;
  }

  .toggle:focus-visible {
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .error {
    min-height: 1.5em;
    color: var(--text-field-error-text);
    font-weight: 600;
  }

  .saved {
    min-height: 1.5em;
    color: var(--ok);
    font-weight: 600;
  }

  .actions {
    display: flex;
  }
</style>
