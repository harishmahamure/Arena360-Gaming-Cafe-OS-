<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { AccountPack, SnapshotLocale } from "@arena/ui/generated";
  import { onMount, tick } from "svelte";
  import { balanceText, packLine, timeLeftText } from "./account-text";
  import { hudExpand, playerAccount, playerEnd, playerLock } from "./bridge";
  import { EXTEND_KEYS } from "./hud";
  import { chipText } from "./shell";

  // The account menu (Story 6.17): a `surface` panel under the top bar with the player's name,
  // wallet and time left, and the rows Balance, Messages, Extend (Story 6.21, prepaid only, shown
  // with its Ctrl+Shift+E key caps), Lock, I'm done (Story 6.18, postpaid only) and Log out. Esc
  // closes it and
  // focus goes back to the chip that opened it. Nothing here decides anything: Balance asks the
  // Hub, Log out asks the Hub to end the player's own session, and Lock is a screen the service
  // covers while the clock goes on.

  let {
    name,
    profiled,
    prepaid,
    seconds,
    balance,
    locale,
    connected,
    onclose,
    onmessages,
    onaccount,
  }: {
    name: string;
    /** A profiled player: Balance and Lock are offered (a guest has no wallet and no password). */
    profiled: boolean;
    /** The clock counts down (prepaid) rather than up (postpaid). */
    prepaid: boolean;
    /** Seconds on the chip: left for prepaid, played for postpaid. */
    seconds: number;
    /** The wallet balance in minor units (profiled players), as the Hub last said. */
    balance: number | null;
    /** The branch's money format, once the attract view has told it. */
    locale: SnapshotLocale | null;
    /** The Hub link is up: Hub-dependent rows say "Counter is reconnecting" when it is not. */
    connected: boolean;
    /** Closes the menu; `true` sends focus back to the chip that opened it. */
    onclose: (refocus: boolean) => void;
    /** "Messages": open the messages bell's list. */
    onmessages: () => void;
    /** "Account": open the Account page (Story 6.34). */
    onaccount: () => void;
  } = $props();

  type View = "menu" | "balance" | "confirm" | "done";
  let view: View = $state("menu");
  let root: HTMLDivElement | undefined = $state();
  let rows: HTMLButtonElement[] = $state([]);
  /** A Hub request is out. */
  let busy = $state(false);
  /** The line under the rows: why the last Log out or Lock did not happen. */
  let line: "reconnecting" | "logout_failed" | "lock_failed" | null = $state(null);
  /** What the Balance view holds: `null` while it loads, `false` when the Hub did not answer. */
  let account: { balanceMinor: number; packs: AccountPack[] } | false | null = $state(null);

  const money = $derived(balance !== null && locale ? balanceText(balance, locale) : null);
  const timeLabel = $derived(
    t(prepaid ? "player.shell.chip_left" : "player.shell.chip_played", {
      time: chipText(seconds),
    }),
  );
  const lineText = $derived(
    line === "reconnecting"
      ? t("player.hub.reconnecting_retry")
      : line === "logout_failed"
        ? t("player.account.logout_failed")
        : line === "lock_failed"
          ? t("player.account.lock_failed")
          : "",
  );

  /** Moves focus into the current view: its first control, or Stay when Log out asks. */
  async function focusFirst() {
    await tick();
    const buttons = root?.querySelectorAll<HTMLElement>(".actions button");
    if ((view === "confirm" || view === "done") && buttons?.length) {
      return buttons[buttons.length - 1].focus();
    }
    (root?.querySelector<HTMLElement>("[data-first]") ?? buttons?.[0])?.focus();
  }

  onMount(() => {
    void focusFirst();
    const away = (e: PointerEvent) => {
      const target = e.target as Element | null;
      if (target?.closest("[data-account-opener]")) return;
      if (root && target && !root.contains(target)) onclose(false);
    };
    window.addEventListener("pointerdown", away);
    return () => window.removeEventListener("pointerdown", away);
  });

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onclose(true);
      return;
    }
    if (view !== "menu" || (event.key !== "ArrowDown" && event.key !== "ArrowUp")) return;
    const live = rows.filter(Boolean);
    const at = live.findIndex((r) => r === document.activeElement);
    const next =
      event.key === "ArrowDown" ? (at + 1) % live.length : (at <= 0 ? live.length : at) - 1;
    event.preventDefault();
    live[next]?.focus();
  }

  async function openBalance() {
    line = null;
    view = "balance";
    account = null;
    void focusFirst();
    if (!connected) {
      account = false;
      return;
    }
    const result = await playerAccount();
    account =
      result.kind === "ok" ? { balanceMinor: result.balanceMinor, packs: result.packs } : false;
  }

  function messages() {
    onclose(false);
    onmessages();
  }

  function account_page() {
    onclose(false);
    onaccount();
  }

  /** "Extend": the HUD expands into its panel with the options, as Ctrl+Shift+E does. */
  function extend() {
    onclose(false);
    hudExpand();
  }

  async function lock() {
    if (busy) return;
    busy = true;
    line = null;
    const locked = await playerLock();
    busy = false;
    if (locked) return onclose(false);
    line = "lock_failed";
  }

  async function logout() {
    line = null;
    if (!connected) {
      line = "reconnecting";
      return;
    }
    // Paid-for time still on the clock: ask in place, once.
    if (prepaid && seconds > 0) {
      view = "confirm";
      void focusFirst();
      return;
    }
    await end();
  }

  /** "I'm done": a postpaid session ends once the player has said yes in place. */
  function done() {
    line = null;
    if (!connected) {
      line = "reconnecting";
      return;
    }
    view = "done";
    void focusFirst();
  }

  async function end() {
    if (busy) return;
    busy = true;
    line = null;
    const outcome = await playerEnd();
    busy = false;
    if (outcome.kind === "ok") return onclose(false);
    line = outcome.kind === "refused" ? "logout_failed" : "reconnecting";
    view = "menu";
    void focusFirst();
  }

  function stay() {
    view = "menu";
    void focusFirst();
  }
</script>

<div
  class="account-menu"
  role="dialog"
  aria-label={t("player.account.menu_label")}
  tabindex="-1"
  bind:this={root}
  onkeydown={onKeydown}
  data-testid="account-menu"
>
  <header class="head">
    <p class="who type-kiosk-title" data-testid="account-menu-name">{name}</p>
    {#if money !== null}
      <p class="fact type-kiosk-body tabular-nums" data-testid="account-menu-balance">
        {t("player.account.header_balance", { balance: money })}
      </p>
    {/if}
    <p class="fact type-kiosk-body tabular-nums" data-testid="account-menu-time">{timeLabel}</p>
  </header>

  {#if view === "menu"}
    <ul class="rows">
      {#if profiled}
        <li>
          <button
            bind:this={rows[0]}
            data-first
            type="button"
            class="row type-kiosk-label"
            data-testid="account-row-balance"
            onclick={openBalance}>{t("player.account.row.balance")}</button
          >
        </li>
      {/if}
      <li>
        <button
          bind:this={rows[1]}
          data-first={profiled ? undefined : ""}
          type="button"
          class="row type-kiosk-label"
          data-testid="account-row-messages"
          onclick={messages}>{t("player.account.row.messages")}</button
        >
      </li>
      {#if profiled}
        <li>
          <button
            bind:this={rows[2]}
            type="button"
            class="row type-kiosk-label"
            data-testid="account-row-account"
            onclick={account_page}>{t("player.account.row.account")}</button
          >
        </li>
      {/if}
      {#if profiled && prepaid}
        <li>
          <button
            bind:this={rows[3]}
            type="button"
            class="row extend type-kiosk-label"
            data-testid="account-row-extend"
            onclick={extend}
          >
            <span>{t("player.account.row.extend")}</span>
            <span
              class="keys"
              role="img"
              aria-label={t("player.warning.hotkey")}
              data-testid="account-row-extend-keys"
            >
              {#each EXTEND_KEYS as key, i (key)}
                {#if i > 0}<span aria-hidden="true">+</span>{/if}
                <kbd class="cap type-kiosk-label" aria-hidden="true">{key}</kbd>
              {/each}
            </span>
          </button>
        </li>
      {/if}
      {#if profiled}
        <li>
          <button
            bind:this={rows[4]}
            type="button"
            class="row type-kiosk-label"
            data-testid="account-row-lock"
            aria-disabled={busy ? "true" : undefined}
            onclick={lock}>{t("player.account.row.lock")}</button
          >
        </li>
      {/if}
      {#if !prepaid}
        <li>
          <button
            bind:this={rows[5]}
            type="button"
            class="row type-kiosk-label"
            data-testid="account-row-done"
            aria-disabled={busy ? "true" : undefined}
            onclick={done}>{t("player.account.row.done")}</button
          >
        </li>
      {/if}
      <li>
        <button
          bind:this={rows[6]}
          type="button"
          class="row type-kiosk-label"
          data-testid="account-row-logout"
          aria-disabled={busy ? "true" : undefined}
          onclick={logout}>{t("player.account.row.logout")}</button
        >
      </li>
    </ul>
    <p
      class="line type-kiosk-label"
      role={lineText ? "alert" : undefined}
      data-testid="account-menu-line"
    >
      {#if lineText}<span aria-hidden="true">⚠</span> {lineText}{/if}
    </p>
  {:else if view === "balance"}
    <section class="panel" aria-label={t("player.account.balance_title")}>
      <h2 class="type-kiosk-label">{t("player.account.balance_title")}</h2>
      {#if account === null}
        <p class="type-kiosk-body" role="status" data-testid="account-balance-loading">
          {t("player.account.loading")}
        </p>
      {:else if account === false || !locale}
        <p class="type-kiosk-body" role="alert" data-testid="account-balance-error">
          {t("player.hub.reconnecting_retry")}
        </p>
      {:else}
        <p class="type-kiosk-body tabular-nums" data-testid="account-balance-wallet">
          {t("player.account.balance_wallet", {
            balance: balanceText(account.balanceMinor, locale),
          })}
        </p>
        <h3 class="type-kiosk-label">{t("player.account.packs_title")}</h3>
        {#if account.packs.length === 0}
          <p class="type-kiosk-body" data-testid="account-balance-no-packs">
            {t("player.account.no_packs")}
          </p>
        {:else}
          <ul class="packs" data-testid="account-balance-packs">
            {#each account.packs as pack, n (n)}
              <li class="type-kiosk-body tabular-nums">{packLine(pack, locale)}</li>
            {/each}
          </ul>
        {/if}
      {/if}
      <p class="type-kiosk-body topups" data-testid="account-balance-topups">
        {t("player.account.topups")}
      </p>
      <div class="actions">
        <Button variant="secondary" label="player.account.back" onclick={stay} />
      </div>
    </section>
  {:else if view === "done"}
    <section class="panel" aria-label={t("player.account.row.done")}>
      <p class="type-kiosk-body" role="alert" data-testid="account-done-confirm">
        {t("player.account.done_confirm")}
      </p>
      <div class="actions">
        <Button
          label="player.account.done_yes"
          disabled={busy}
          disabledReason="player.unlock.checking"
          onclick={end}
        />
        <Button variant="secondary" label="player.account.done_cancel" onclick={stay} />
      </div>
    </section>
  {:else}
    <section class="panel" aria-label={t("player.account.row.logout")}>
      <p class="type-kiosk-body" role="alert" data-testid="account-logout-confirm">
        {t("player.account.logout_confirm", { time: timeLeftText(seconds) })}
      </p>
      <div class="actions">
        <Button
          label="player.account.logout_yes"
          disabled={busy}
          disabledReason="player.unlock.checking"
          onclick={end}
        />
        <Button variant="secondary" label="player.account.logout_cancel" onclick={stay} />
      </div>
    </section>
  {/if}
</div>

<style>
  .account-menu {
    position: absolute;
    top: calc(100% + var(--space-2));
    right: 0;
    z-index: 6;
    display: grid;
    gap: var(--space-3);
    box-sizing: border-box;
    width: 360px;
    max-height: 80vh;
    padding: var(--space-4);
    overflow-y: auto;
    border: var(--account-menu-border);
    border-radius: var(--radius-xl);
    background-color: var(--account-menu-background);
    color: var(--account-menu-foreground);
    box-shadow: var(--elevation-menu);
    text-align: left;
  }

  .account-menu:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .account-menu p,
  .account-menu h2,
  .account-menu h3 {
    margin: 0;
  }

  .head {
    display: grid;
    gap: var(--space-1);
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--line);
  }

  .who {
    overflow-wrap: anywhere;
  }

  .fact {
    color: var(--text-muted);
  }

  .rows,
  .packs {
    display: grid;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    width: 100%;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 0;
    border-radius: var(--radius-full);
    background: transparent;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }

  .row:hover {
    background-color: var(--surface-2);
  }

  /* Extend names the key that does the same: Ctrl+Shift+E, as caps at the row's end. */
  .row.extend {
    justify-content: space-between;
    gap: var(--space-4);
  }

  .keys {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .cap {
    padding: 0 var(--space-2);
    border: 2px solid var(--border-strong);
    border-radius: var(--radius-md);
  }

  .row:focus-visible,
  .row:focus {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .line {
    min-height: 1.5em;
    color: var(--warning);
  }

  .panel {
    display: grid;
    gap: var(--space-3);
  }

  .panel h2,
  .panel h3 {
    color: var(--text-muted);
  }

  .topups {
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
