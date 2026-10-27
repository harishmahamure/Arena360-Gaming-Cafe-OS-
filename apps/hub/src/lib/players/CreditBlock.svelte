<script lang="ts">
  import { Button, TextField, focusFirstInvalid, formatMoney, t } from "@arena/ui";
  import type { CreditCollection, CreditView, SessionStaff } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import {
    SESSION_DEPENDENCY,
    hasPermission,
    loadWallet,
    newIdempotencyKey,
    submitIntent,
  } from "../auth";
  import { branchLocale } from "../locale.svelte";
  import { amountText, headroomOf, parseAmount } from "../settle";
  import { applyWalletChanged, walletEvents } from "../wallet.svelte";
  import CollectCredit from "./CollectCredit.svelte";
  import CollectionSummary from "./CollectionSummary.svelte";

  let { playerId, staff }: { playerId: string; staff: SessionStaff | null } = $props();

  const DEFAULT_DAYS = "3, 7, 14";
  const locale = $derived(branchLocale.current);
  const canManage = $derived(hasPermission(staff, "credit.manage"));
  const canCollect = $derived(hasPermission(staff, "credit.collect"));

  let credit = $state<CreditView | null>(null);
  let walletMinor = $state(0);
  let collecting = $state(false);
  let collection: CreditCollection | null = $state(null);
  let loaded = $state(false);
  let failed = $state(false);
  let editing = $state(false);
  let limitText = $state("");
  let daysText = $state(DEFAULT_DAYS);
  let limitError: MessageKey | null = $state(null);
  let daysError: MessageKey | null = $state(null);
  let formError: MessageKey | null = $state(null);
  let saved = $state(false);
  let busy = $state(false);
  let root: HTMLElement | undefined = $state();
  const canCollectNow = $derived(canCollect && (credit?.outstanding_minor ?? 0) > 0 && !editing);
  let turn = 0;

  const money = (minor: number) => (locale ? formatMoney(minor, locale) : "");

  async function refresh() {
    const mine = ++turn;
    const outcome = await loadWallet(playerId);
    if (mine !== turn) return;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    failed = outcome.kind !== "ok";
    if (outcome.kind === "ok") {
      credit = outcome.value.credit ?? null;
      walletMinor = outcome.value.balance_minor;
    }
    loaded = true;
  }

  $effect(() => {
    void playerId;
    untrack(() => void refresh());
  });

  $effect(() => {
    const changed = walletEvents.version;
    if (changed > 0 && walletEvents.playerId === untrack(() => playerId)) {
      untrack(() => void refresh());
    }
  });

  function parseDays(text: string): number[] | null {
    const parts = text
      .split(/[\s,]+/)
      .filter((p) => p !== "")
      .map((p) => (/^\d{1,3}$/.test(p) ? Number(p) : NaN));
    if (parts.length < 1 || parts.length > 10) return null;
    if (parts.some((d) => !Number.isInteger(d) || d < 1 || d > 365)) return null;
    return [...new Set(parts)].sort((a, b) => a - b);
  }

  function open() {
    saved = false;
    formError = null;
    limitError = null;
    daysError = null;
    limitText = credit && locale ? amountText(credit.limit_minor, locale.exponent) : "";
    daysText = credit ? credit.reminder_days.join(", ") : DEFAULT_DAYS;
    editing = true;
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (busy || !locale) return;
    const limit = parseAmount(limitText, locale.exponent);
    const days = parseDays(daysText);
    limitError = limit === null ? "hub.credit.err.limit" : null;
    daysError = days === null ? "hub.credit.err.days" : null;
    formError = null;
    if (limit === null || days === null) {
      await tick();
      return focusFirstInvalid(root);
    }
    busy = true;
    const outcome = await submitIntent(
      { kind: "credit.set_limit", player_id: playerId, limit_minor: limit, reminder_days: days },
      newIdempotencyKey(),
    ).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      editing = false;
      saved = true;
      return void (await refresh());
    }
    if (outcome.kind === "unreachable") formError = "hub.credit.failed";
    else if (outcome.error.code === "CREDIT_BELOW_OUTSTANDING") limitError = "hub.credit.err.below";
    else formError = "hub.credit.refused";
    await tick();
    if (limitError) focusFirstInvalid(root);
  }
</script>

<section class="credit" aria-labelledby="credit-title" data-testid="credit" bind:this={root}>
  <h2 class="type-hub-title" id="credit-title">{t("hub.credit.title")}</h2>
  {#if failed}
    <p class="type-hub-label" role="alert">
      <span aria-hidden="true">⚠</span>
      {t("hub.credit.load_failed")}
    </p>
  {:else if loaded && !credit}
    <p class="type-hub-body" data-testid="credit-off">{t("hub.credit.off")}</p>
  {:else if credit}
    <dl class="type-hub-body" data-testid="credit-summary">
      <dt>{t("hub.credit.limit")}</dt>
      <dd class="tabular-nums" data-testid="credit-limit">{money(credit.limit_minor)}</dd>
      <dt>{t("hub.credit.outstanding")}</dt>
      <dd class="tabular-nums" data-testid="credit-outstanding">
        {money(credit.outstanding_minor)}
      </dd>
      <dt>{t("hub.credit.headroom")}</dt>
      <dd class="tabular-nums" data-testid="credit-headroom">{money(headroomOf(credit))}</dd>
      <dt>{t("hub.credit.days")}</dt>
      <dd data-testid="credit-days">{credit.reminder_days.join(", ")}</dd>
    </dl>
  {/if}

  {#if saved}
    <p class="type-hub-label" role="status" data-testid="credit-saved">{t("hub.credit.saved")}</p>
  {/if}

  {#if editing}
    <form onsubmit={save} novalidate data-testid="credit-form">
      <TextField
        label="hub.credit.limit_field"
        bind:value={limitText}
        inputmode="decimal"
        autocomplete="off"
        error={limitError}
      />
      <TextField
        label="hub.credit.days_field"
        bind:value={daysText}
        autocomplete="off"
        error={daysError}
      />
      {#if formError}
        <p class="type-hub-label" role="alert" data-testid="credit-error">
          <span aria-hidden="true">⚠</span>
          {t(formError)}
        </p>
      {/if}
      <div class="actions">
        <Button
          label="hub.credit.save"
          type="submit"
          disabled={busy}
          disabledReason="hub.sheet.waiting"
        />
        <Button label="hub.credit.cancel" variant="secondary" onclick={() => (editing = false)} />
      </div>
    </form>
  {:else if loaded && !failed && !collecting && (canManage || canCollectNow)}
    <div class="actions" data-testid="credit-actions">
      {#if canManage}
        <Button label={credit ? "hub.credit.edit" : "hub.credit.enable"} onclick={open} />
      {/if}
      {#if canCollectNow}
        <Button
          label="hub.credit.collect"
          onclick={() => {
            collection = null;
            collecting = true;
          }}
        />
      {/if}
    </div>
  {/if}

  {#if collecting && credit}
    <CollectCredit
      {playerId}
      outstanding={credit.outstanding_minor}
      {walletMinor}
      ondone={(done) => {
        collecting = false;
        collection = done;
        applyWalletChanged(playerId);
      }}
      oncancel={() => (collecting = false)}
    />
  {:else if collection}
    <CollectionSummary {collection} onclose={() => (collection = null)} />
  {/if}
</section>

<style>
  .credit {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  p {
    margin: 0;
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-1) var(--space-4);
    margin: 0;
  }

  dd {
    margin: 0;
  }

  form {
    display: grid;
    gap: var(--space-3);
    max-width: 24rem;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
</style>
