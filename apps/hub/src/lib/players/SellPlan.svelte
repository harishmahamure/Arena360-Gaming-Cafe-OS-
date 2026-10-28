<script lang="ts">
  import { Button, formatMoney, t } from "@arena/ui";
  import type {
    BusinessBuyer,
    PaymentRow,
    PlanView,
    ReceiptChoice,
    SnapshotBill,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { SESSION_DEPENDENCY, loadWallet, newIdempotencyKey, submitIntent } from "../auth";
  import { branchLocale } from "../locale.svelte";
  import { snapshotOf } from "../operating";
  import { loadPlans, termsText } from "../plans";
  import SettleBar from "../SettleBar.svelte";
  import { offerUndo } from "../undo.svelte";

  let {
    playerId,
    ondone,
    oncancel,
  }: {
    playerId: string;
    ondone: () => void;
    oncancel: () => void;
  } = $props();

  const KIND: Record<string, MessageKey> = {
    pack: "hub.plans.kind.pack",
    pass: "hub.plans.kind.pass",
  };
  const REFUSAL: Record<string, MessageKey> = {
    PASS_ALREADY_ACTIVE: "hub.plans.sell.pass_active",
    PAYMENT_SHORT: "hub.settle.short",
    INSUFFICIENT_BALANCE: "hub.settle.insufficient",
    CREDIT_LIMIT: "hub.settle.credit_limit",
    CREDIT_PAUSED: "hub.plans.sell.credit_paused",
    REFERENCE_REQUIRED: "hub.settle.reference",
  };

  const locale = $derived(branchLocale.current);
  const groups = $derived(snapshotOf(page.data)?.groups ?? []);
  const groupName = (id: string) => groups.find((g) => g.id === id)?.name ?? id;

  let plans: PlanView[] = $state([]);
  let loading = $state(true);
  let loadFailed = $state(false);
  let chosen = $state(null) as PlanView | null;
  let walletMinor = $state(0);
  let credit: SnapshotBill["credit"] = $state(null);
  let error: MessageKey | null = $state(null);
  let busy = $state(false);
  let attempt: { key: string; payments: unknown } | null = null;

  const offered = $derived(plans.filter((p) => p.active && p.plan_kind !== "block"));
  const bill = $derived<SnapshotBill>({
    session_id: "",
    device_id: "",
    lines: [],
    subtotal_minor: chosen?.price_minor ?? 0,
    paid_minor: 0,
    remaining_minor: chosen?.price_minor ?? 0,
    wallet_minor: walletMinor,
    credit,
    player_id: playerId,
  });

  onMount(() => {
    void (async () => {
      const [list, wallet] = await Promise.all([loadPlans(), loadWallet(playerId)]);
      if (list.kind === "signed_out" || wallet.kind === "signed_out") {
        return void (await invalidate(SESSION_DEPENDENCY));
      }
      loadFailed = list.kind !== "ok";
      if (list.kind === "ok") plans = list.plans;
      if (wallet.kind === "ok") {
        walletMinor = wallet.value.balance_minor;
        credit = wallet.value.credit ?? null;
      }
      loading = false;
    })();
  });

  async function settle(payments: PaymentRow[], receipt?: ReceiptChoice, business?: BusinessBuyer) {
    if (busy || !chosen) return;
    busy = true;
    error = null;
    const same =
      attempt && JSON.stringify(attempt.payments) === JSON.stringify([payments, receipt, business]);
    const key = same && attempt ? attempt.key : newIdempotencyKey();
    attempt = { key, payments: [payments, receipt, business] };
    const outcome = await submitIntent(
      { kind: "plan.sell", player_id: playerId, plan_id: chosen.id, payments, receipt, business },
      key,
    ).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      offerUndo(outcome.result, "plan");
      return ondone();
    }
    error =
      outcome.kind === "error"
        ? (REFUSAL[outcome.error.code] ?? "hub.plans.sell.refused")
        : "hub.plans.sell.failed";
  }
</script>

<section class="sell" aria-labelledby="sell-title" data-testid="sell-plan">
  {#if chosen === null}
    <h2 class="type-hub-title" id="sell-title">{t("hub.plans.sell.title")}</h2>
    {#if loading}
      <p class="type-hub-body" role="status">{t("hub.plans.sell.loading")}</p>
    {:else if loadFailed}
      <p class="type-hub-label" role="alert">
        <span aria-hidden="true">⚠</span>
        {t("hub.plans.load_failed")}
      </p>
    {:else if offered.length === 0}
      <p class="type-hub-body" data-testid="sell-none">{t("hub.plans.sell.none")}</p>
    {:else}
      <ul class="plans" data-testid="sell-list">
        {#each offered as p (p.id)}
          <li class="plan">
            <span class="type-hub-title">{p.name}</span>
            <span class="badge type-hub-label">{t(KIND[p.plan_kind] ?? "hub.plans.kind.pack")}</span
            >
            <span class="type-hub-label tabular-nums">
              {locale ? formatMoney(p.price_minor, locale) : ""}
            </span>
            <span class="type-hub-label">{termsText(p)}</span>
            <span class="type-hub-label">
              {t("hub.plans.for_groups", { names: p.group_ids.map(groupName).join(", ") })}
            </span>
            <button
              type="button"
              class="ui-interactive btn type-hub-label"
              data-testid="sell-pick"
              onclick={() => (chosen = p)}>{t("hub.plans.sell.pick", { name: p.name })}</button
            >
          </li>
        {/each}
      </ul>
    {/if}
    <div class="actions">
      <Button label="hub.plans.sell.cancel" variant="secondary" onclick={oncancel} />
    </div>
  {:else if locale}
    <SettleBar
      {bill}
      {locale}
      {busy}
      onsettle={(p, receipt, business) => void settle(p, receipt, business)}
      title="hub.plans.sell.title"
      receiptChoice
    >
      {#snippet extra()}
        <p class="type-hub-body" data-testid="sell-line">
          {chosen?.name} · {formatMoney(chosen?.price_minor ?? 0, locale)}
        </p>
      {/snippet}
    </SettleBar>
    {#if error}
      <p class="type-hub-label" role="alert" data-testid="sell-error">
        <span aria-hidden="true">⚠</span>
        {t(error)}
      </p>
    {/if}
    <div class="actions">
      <Button
        label="hub.plans.sell.back"
        variant="secondary"
        onclick={() => {
          chosen = null;
          error = null;
        }}
      />
      <Button label="hub.plans.sell.cancel" variant="secondary" onclick={oncancel} />
    </div>
  {/if}
</section>

<style>
  .sell {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  p {
    margin: 0;
  }

  .plans {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
    max-width: 48rem;
  }

  .plan {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .badge {
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
  }

  .btn {
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--surface-2);
    color: var(--text);
    cursor: pointer;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
</style>
