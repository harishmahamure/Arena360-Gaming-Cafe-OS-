<script lang="ts">
  import { Button, TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { CreditCollection, PaymentRow, SnapshotBill } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, newIdempotencyKey, submitIntent } from "../auth";
  import { branchLocale } from "../locale.svelte";
  import SettleBar from "../SettleBar.svelte";
  import { amountText, parseAmount } from "../settle";
  import { offerUndo } from "../undo.svelte";

  let {
    playerId,
    outstanding,
    walletMinor,
    ondone,
    oncancel,
  }: {
    playerId: string;
    outstanding: number;
    walletMinor: number;
    ondone: (collection: CreditCollection | null) => void;
    oncancel: () => void;
  } = $props();

  const locale = $derived(branchLocale.current);
  let amountField = $state(
    untrack(() =>
      branchLocale.current ? amountText(outstanding, branchLocale.current.exponent) : "",
    ),
  );
  let amount: number | null = $state(null);
  let error: MessageKey | null = $state(null);
  let busy = $state(false);
  let root: HTMLElement | undefined = $state();
  let attempt: { key: string; payments: PaymentRow[] } | null = null;

  const bill = $derived<SnapshotBill>({
    session_id: "",
    device_id: "",
    lines: [],
    subtotal_minor: amount ?? 0,
    paid_minor: 0,
    remaining_minor: amount ?? 0,
    wallet_minor: walletMinor,
  });

  async function next() {
    if (!locale) return;
    const parsed = parseAmount(amountField, locale.exponent);
    if (parsed === null) error = "hub.credit.collect.err.amount";
    else if (parsed > outstanding) error = "hub.credit.collect.err.over";
    else error = null;
    if (error || parsed === null) {
      await tick();
      return focusFirstInvalid(root);
    }
    amount = parsed;
  }

  async function settle(payments: PaymentRow[]) {
    if (busy || amount === null) return;
    busy = true;
    error = null;
    const same = attempt && JSON.stringify(attempt.payments) === JSON.stringify(payments);
    const key = same && attempt ? attempt.key : newIdempotencyKey();
    attempt = { key, payments };
    const outcome = await submitIntent(
      { kind: "credit.collect", player_id: playerId, amount_minor: amount, payments },
      key,
    ).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      offerUndo(outcome.result, "collect");
      return ondone(outcome.result.settled?.collection ?? null);
    }
    error = outcome.kind === "error" ? "hub.credit.collect.refused" : "hub.credit.collect.failed";
  }
</script>

<section class="collect" aria-labelledby="collect-title" data-testid="collect" bind:this={root}>
  {#if amount === null}
    <h3 class="type-hub-title" id="collect-title">{t("hub.credit.collect.title")}</h3>
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void next();
      }}
      novalidate
    >
      <TextField
        label="hub.credit.collect.amount"
        bind:value={amountField}
        inputmode="decimal"
        autocomplete="off"
        {error}
      />
      <div class="actions">
        <Button label="hub.credit.collect.continue" type="submit" />
        <Button label="hub.credit.collect.cancel" variant="secondary" onclick={oncancel} />
      </div>
    </form>
  {:else if locale}
    <SettleBar
      {bill}
      {locale}
      {busy}
      onsettle={(p) => void settle(p)}
      title="hub.credit.collect.title"
    />
    {#if error}
      <p class="type-hub-label" role="alert" data-testid="collect-error">
        <span aria-hidden="true">⚠</span>
        {t(error)}
      </p>
    {/if}
    <div class="actions">
      <Button label="hub.credit.collect.cancel" variant="secondary" onclick={oncancel} />
    </div>
  {/if}
</section>

<style>
  .collect {
    display: grid;
    gap: var(--space-3);
  }

  h3,
  p {
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
