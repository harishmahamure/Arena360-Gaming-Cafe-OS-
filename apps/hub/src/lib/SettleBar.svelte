<script lang="ts">
  import {
    Button,
    TextField,
    focusFirstInvalid,
    formatMoney,
    registerAdExclusion,
    t,
  } from "@arena/ui";
  import type {
    PaymentMethod,
    PaymentRow,
    SnapshotBill,
    SnapshotLocale,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick, type Snippet } from "svelte";
  import { cashBlocked } from "./shift.svelte";
  import {
    amountText,
    creditOffered,
    headroomOf,
    methodsFor,
    rowErrors,
    rowsValid,
    tally,
    toPayments,
    type Row,
  } from "./settle";

  let {
    bill,
    locale,
    busy,
    onsettle,
    title = "hub.settle.title",
    extra,
  }: {
    bill: SnapshotBill;
    locale: SnapshotLocale;
    busy: boolean;
    onsettle: (payments: PaymentRow[]) => void;
    title?: MessageKey;
    extra?: Snippet;
  } = $props();

  let root: HTMLElement | undefined = $state();
  // A bill being settled is an ad-exclusion surface (Story 4.13).
  $effect(() => registerAdExclusion("settle-bar"));
  let rows: Row[] = $state([]);
  let attempted = $state(false);

  const id = $props.id();
  const exponent = $derived(locale.exponent);
  const total = $derived(bill.remaining_minor);
  const sums = $derived(tally(total, rows, exponent));
  const wallet = $derived(bill.wallet_minor ?? 0);
  // Cash needs an open shift; every other method settles without one.
  const noCash = $derived(cashBlocked());
  const methods = $derived(
    methodsFor(bill.wallet_minor, bill.credit).filter((m) => !(noCash && m === "cash")),
  );
  const credit = $derived(creditOffered(bill.credit) ? bill.credit : null);
  const closed = $derived(sums.remaining === 0 && !sums.overpaid);
  const money = (minor: number) => formatMoney(minor, locale);

  const errorOf = (row: Row, field: "amount" | "reference"): MessageKey | null =>
    attempted ? (rowErrors(row, exponent, wallet)[field] ?? null) : null;

  const AMOUNT: Record<PaymentMethod, MessageKey> = {
    cash: "hub.settle.amount.cash",
    upi: "hub.settle.amount.upi",
    card: "hub.settle.amount.card",
    wallet: "hub.settle.amount.wallet",
    credit: "hub.settle.amount.credit",
  };
  const REFERENCE: Record<PaymentMethod, MessageKey> = {
    cash: "hub.settle.reference.upi",
    upi: "hub.settle.reference.upi",
    card: "hub.settle.reference.card",
    wallet: "hub.settle.reference.upi",
    credit: "hub.settle.reference.upi",
  };
  const NAME: Record<PaymentMethod, MessageKey> = {
    cash: "hub.settle.method.cash",
    upi: "hub.settle.method.upi",
    card: "hub.settle.method.card",
    wallet: "hub.settle.method.wallet",
    credit: "hub.settle.method.credit",
  };

  async function choose(method: PaymentMethod) {
    if (!rows.some((r) => r.method === method)) {
      const suggested =
        method === "wallet"
          ? Math.min(wallet, sums.remaining)
          : method === "credit" && credit && headroomOf(credit) > 0
            ? Math.min(headroomOf(credit), sums.remaining)
            : sums.remaining;
      rows.push({ method, amount: amountText(suggested, exponent), reference: "" });
    }
    await tick();
    const input = root?.querySelector<HTMLInputElement>(`[data-row="${method}"] input`);
    input?.focus();
    input?.select();
  }

  function drop(method: PaymentMethod) {
    rows = rows.filter((r) => r.method !== method);
  }

  function arrows(event: KeyboardEvent) {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (step === 0) return;
    const buttons = [...(root?.querySelectorAll<HTMLElement>("[data-method]") ?? [])];
    const at = buttons.indexOf(document.activeElement as HTMLElement);
    if (at < 0) return;
    event.preventDefault();
    buttons[(at + step + buttons.length) % buttons.length]?.focus();
  }

  export async function submit() {
    if (busy || !closed) return;
    attempted = true;
    if (!rowsValid(rows, exponent, wallet)) {
      await tick();
      focusFirstInvalid(root);
      return;
    }
    onsettle(toPayments(rows, sums.change, exponent));
  }
</script>

<section
  class="bar"
  data-component="settle-bar"
  data-testid="settle-bar"
  aria-labelledby="{id}-title"
  bind:this={root}
>
  <h3 class="type-hub-title" id="{id}-title">{t(title)}</h3>
  <p class="total">
    <span class="type-hub-label">{t("hub.settle.total")}</span>
    <span class="type-hub-numeric tabular-nums" data-testid="settle-total">{money(total)}</span>
  </p>

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="methods"
    role="group"
    aria-label={t("hub.settle.methods")}
    onkeydown={arrows}
    data-testid="settle-methods"
  >
    {#each methods as method (method)}
      <button
        type="button"
        class="ui-interactive method type-hub-label"
        data-method={method}
        aria-pressed={rows.some((r) => r.method === method)}
        disabled={busy}
        onclick={() => void choose(method)}>{t(NAME[method])}</button
      >
    {/each}
  </div>

  {#if noCash}
    <p class="type-hub-label shift-closed" data-testid="settle-shift-closed">
      <span aria-hidden="true">⚠</span>
      {t("hub.settle.shift_closed")}
      <a class="shift-link" href="/shift">{t("hub.settle.shift_link")}</a>
    </p>
  {/if}

  {@render extra?.()}

  {#if rows.length > 0}
    <div class="split" data-component="payment-split" data-testid="payment-split">
      {#each rows as row (row.method)}
        <div class="row" data-row={row.method} data-testid="split-row">
          <TextField
            label={AMOUNT[row.method]}
            bind:value={row.amount}
            inputmode="decimal"
            autocomplete="off"
            error={errorOf(row, "amount")}
          />
          {#if row.method === "wallet"}
            <p class="type-hub-label wallet-balance tabular-nums" data-testid="wallet-balance">
              {t("hub.settle.wallet_balance", { amount: money(wallet) })}
            </p>
          {:else if row.method === "credit" && credit}
            <p class="type-hub-label wallet-balance tabular-nums" data-testid="credit-info">
              {t("hub.settle.credit_info", {
                outstanding: money(credit.outstanding_minor),
                limit: money(credit.limit_minor),
                headroom: money(headroomOf(credit)),
              })}
            </p>
          {:else if row.method !== "cash"}
            <div data-reference>
              <TextField
                label={REFERENCE[row.method]}
                bind:value={row.reference}
                inputmode={row.method === "upi" ? "numeric" : "text"}
                autocomplete="off"
                error={errorOf(row, "reference")}
              />
            </div>
          {/if}
          <button
            type="button"
            class="ui-interactive remove type-hub-label"
            disabled={busy}
            onclick={() => drop(row.method)}
            >{t("hub.settle.remove", { method: t(NAME[row.method]) })}</button
          >
        </div>
      {/each}
    </div>
  {/if}

  <div class="remainder type-hub-numeric tabular-nums" role="status" data-testid="settle-remainder">
    {#if sums.overpaid}
      <span class="open">⚠ {t("hub.settle.over")}</span>
    {:else if sums.remaining > 0}
      <span class="open">⚠ {t("hub.settle.remaining", { amount: money(sums.remaining) })}</span>
    {:else}
      <span class="zero">✓ {t("hub.settle.paid_full")}</span>
    {/if}
    {#if sums.change > 0}
      <span data-testid="settle-change"
        >{t("hub.settle.change", { amount: money(sums.change) })}</span
      >
    {/if}
  </div>

  <Button
    label="hub.settle.submit"
    disabled={busy || !closed}
    disabledReason={busy
      ? "hub.sheet.waiting"
      : sums.overpaid
        ? "hub.settle.over"
        : "hub.settle.remaining"}
    reasonParams={{ amount: money(sums.remaining) }}
    onclick={() => void submit()}
  />
</section>

<style>
  .bar {
    display: grid;
    gap: var(--space-3);
    width: 100%;
    box-sizing: border-box;
    padding: var(--space-4) var(--space-hub-panel-padding);
    border-top: var(--settle-bar-border-top);
    background-color: var(--settle-bar-background);
    color: var(--text);
  }

  :global([data-theme="hc"]) .bar {
    border-top: var(--settle-bar-border-top-hc);
    background-color: var(--settle-bar-background-hc);
  }

  h3,
  p {
    margin: 0;
  }

  .total {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .methods {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .method,
  .remove {
    --ui-bg: var(--button-secondary-background);
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    color: var(--button-secondary-foreground);
    font-weight: var(--type-hub-title-weight);
    cursor: pointer;
  }

  .method[aria-pressed="true"] {
    border-width: 2px;
    border-color: var(--accent);
  }

  .shift-closed {
    color: var(--text);
  }

  .shift-link {
    color: var(--text);
    text-decoration: underline;
  }

  .split {
    display: grid;
    gap: var(--space-3);
  }

  .row {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(10em, 1fr));
    align-items: start;
    gap: var(--space-3);
  }

  .remove {
    justify-self: start;
    align-self: center;
  }

  .wallet-balance {
    margin: 0;
    align-self: center;
    color: var(--text-muted);
  }

  .remainder {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .open {
    color: var(--payment-split-remainder-open);
  }

  .zero {
    color: var(--payment-split-remainder-zero);
  }

  :global([data-theme="hc"]) .open {
    color: var(--payment-split-remainder-open-hc);
  }

  :global([data-theme="hc"]) .zero {
    color: var(--payment-split-remainder-zero-hc);
  }
</style>
