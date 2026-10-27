<script lang="ts">
  import { Button, TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { PaymentRow, SnapshotBill } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, newIdempotencyKey, submitIntent } from "../auth";
  import { branchLocale } from "../locale.svelte";
  import SettleBar from "../SettleBar.svelte";
  import { parseAmount } from "../settle";
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

  const locale = $derived(branchLocale.current);
  let amountText = $state("");
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
  });

  async function next() {
    if (!locale) return;
    const parsed = parseAmount(amountText, locale.exponent);
    if (parsed === null) {
      error = "hub.wallet.topup.err.amount";
      await tick();
      return focusFirstInvalid(root);
    }
    error = null;
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
      { kind: "wallet.topup", player_id: playerId, amount_minor: amount, payments },
      key,
    ).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      offerUndo(outcome.result, "topup");
      return ondone();
    }
    error = outcome.kind === "error" ? "hub.wallet.topup.refused" : "hub.wallet.topup.failed";
  }
</script>

<section class="topup" aria-labelledby="topup-title" data-testid="topup" bind:this={root}>
  {#if amount === null}
    <h2 class="type-hub-title" id="topup-title">{t("hub.wallet.topup.title")}</h2>
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void next();
      }}
      novalidate
    >
      <TextField
        label="hub.wallet.topup.amount"
        bind:value={amountText}
        inputmode="decimal"
        autocomplete="off"
        {error}
      />
      <div class="actions">
        <Button label="hub.wallet.topup.continue" type="submit" />
        <Button label="hub.wallet.topup.cancel" variant="secondary" onclick={oncancel} />
      </div>
    </form>
  {:else if locale}
    <SettleBar
      {bill}
      {locale}
      {busy}
      onsettle={(p) => void settle(p)}
      title="hub.wallet.topup.title"
    />
    {#if error}
      <p class="type-hub-label" role="alert" data-testid="topup-error">
        <span aria-hidden="true">⚠</span>
        {t(error)}
      </p>
    {/if}
    <div class="actions">
      <Button label="hub.wallet.topup.cancel" variant="secondary" onclick={oncancel} />
    </div>
  {/if}
</section>

<style>
  .topup {
    display: grid;
    gap: var(--space-3);
  }

  h2,
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
