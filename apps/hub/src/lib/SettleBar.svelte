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
    BusinessBuyer,
    CouponPreview,
    DiscountPreview,
    PaymentMethod,
    PaymentRow,
    PrinterReady,
    ReceiptChoice,
    SnapshotBill,
    SnapshotLocale,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick, untrack, type Snippet } from "svelte";
  import { buyerErrors, toBuyer } from "./business";
  import { normaliseCode, previewCoupon, type CouponSource } from "./coupons";
  import DiscountEntry from "./DiscountEntry.svelte";
  import { TICKET_FRESH_MS, previewDiscount, settleKind, type AppliedDiscount } from "./discount";
  import { dropElevation } from "./auth";
  import { printerReady } from "./printer";
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
    blockedReason,
    onsettle,
    title = "hub.settle.title",
    extra,
    prefillCash = false,
    hotkey = false,
    receiptChoice = false,
    couponSource = null,
    discountDirect = false,
  }: {
    bill: SnapshotBill;
    locale: SnapshotLocale;
    busy: boolean;
    /** Why the bill cannot settle yet (placed items still to serve), shown on the button. */
    blockedReason?: MessageKey;
    /**
     * `receipt` is the choice on the bar, present only when `receiptChoice` is set; `business`
     * is the buyer of a business invoice, present only when staff chose one (Story 5.20).
     * `coupon` is the applied coupon code, present only when staff applied one (Story 5.21).
     * `discount` is the manual discount staff chose, with the manager's elevation ticket when the
     * signed-in member does not hold `discount.manual` themselves (Story 5.22).
     */
    onsettle: (
      payments: PaymentRow[],
      receipt?: ReceiptChoice,
      business?: BusinessBuyer,
      coupon?: string,
      discount?: AppliedDiscount,
    ) => void;
    title?: MessageKey;
    extra?: Snippet;
    /** Starts with one Cash row for the whole total (only while cash can be taken). */
    prefillCash?: boolean;
    /** Ctrl+Enter settles from anywhere on the screen. */
    hotkey?: boolean;
    /** Offers the receipt choice Print · None (a sale; a top-up or a collection has none). */
    receiptChoice?: boolean;
    /** The bill a coupon is priced on; when set the bar offers a coupon field (Story 5.21). */
    couponSource?: CouponSource | null;
    /** The signed-in member holds `discount.manual`: a manual discount needs no PIN (Story 5.22). */
    discountDirect?: boolean;
  } = $props();

  let root: HTMLElement | undefined = $state();
  // A bill being settled is an ad-exclusion surface (Story 4.13).
  $effect(() => registerAdExclusion("settle-bar"));
  let rows: Row[] = $state([]);
  let attempted = $state(false);
  // Whether the printer is ready, asked of the Hub when the bar opens (Story 5.17). A bar with no
  // answer offers no Print: the printer never blocks a settle.
  let printer = $state(null as PrinterReady | null);
  let printerAsked = $state(false);
  let picked: ReceiptChoice | null = $state(null);
  const printable = $derived(printer?.ready === true);
  // WhatsApp is offered for a profiled player with current consent on a paid branch (Story 5.19).
  const whatsapp = $derived(printer?.whatsapp === true);
  const choices = $derived<ReceiptChoice[]>([
    ...(printable ? (["print"] as const) : []),
    "none",
    ...(whatsapp ? (["whatsapp"] as const) : []),
  ]);
  // "Business invoice" is offered on a GST-registered Indian branch (Story 5.20): the customer's
  // GSTIN and legal name go on the invoice. Settle stays disabled until they are valid or it is off.
  const businessOffered = $derived(receiptChoice && printer?.business === true);
  let businessOn = $state(false);
  let buyerGstin = $state("");
  let buyerName = $state("");
  const buyerProblems = $derived(buyerErrors(buyerGstin, buyerName));
  const businessBlocked = $derived(
    businessOffered && businessOn && Object.keys(buyerProblems).length > 0,
  );
  const buyerShown = $derived(buyerGstin.length > 0 || buyerName.length > 0 || attempted);
  const receipt: ReceiptChoice = $derived(
    picked && choices.includes(picked)
      ? picked
      : printable
        ? (printer?.default_choice ?? "print")
        : "none",
  );

  $effect(() => {
    if (!receiptChoice) return;
    let live = true;
    void printerReady(bill.player_id).then((answer) => {
      if (!live) return;
      printer = answer.kind === "ok" ? answer.body : null;
      printerAsked = true;
    });
    return () => {
      live = false;
    };
  });

  // A coupon applied to the bill (Story 5.21): the Hub prices it and the bar shows its line and
  // the new total. It is priced again whenever the bill changes, and dropped with the Hub's reason
  // when it no longer applies.
  let couponText = $state("");
  let applied = $state(null as CouponPreview | null);
  let couponProblem = $state(null as string | null);
  let couponBusy = $state(false);
  let couponOpen = $state(false);
  let couponRuns = 0;
  const couponKey = $derived(JSON.stringify([couponSource, bill.remaining_minor]));

  async function priceCoupon(code: string, source: CouponSource) {
    const mine = ++couponRuns;
    couponBusy = true;
    const answer = await previewCoupon(code, source).finally(() => {
      if (mine === couponRuns) couponBusy = false;
    });
    if (mine !== couponRuns) return;
    if (answer.kind === "ok") {
      applied = answer.preview;
      couponProblem = null;
    } else {
      applied = null;
      couponOpen = true;
      couponProblem =
        answer.kind === "refused" ? answer.message : t("hub.settle.coupon.unreachable");
    }
  }

  function applyCoupon() {
    const code = normaliseCode(couponText);
    if (!couponSource || couponBusy || code.length === 0) return;
    void priceCoupon(code, couponSource);
  }

  function removeCoupon() {
    couponRuns += 1;
    couponBusy = false;
    applied = null;
    couponProblem = null;
    couponText = "";
    couponOpen = false;
  }

  $effect(() => {
    void couponKey;
    untrack(() => {
      const code = applied?.code;
      if (code && couponSource) void priceCoupon(code, couponSource);
    });
  });

  // A manual discount (Story 5.22): staff enter a percent or an amount and a reason, and a manager's
  // PIN approves it unless the signed-in member holds `discount.manual`. The Hub prices it after the
  // coupon; the ticket the PIN earned is handed to the settle once and then forgotten, so a retry
  // asks for the PIN again.
  let discountOpen = $state(false);
  let discountApplied = $state(null as AppliedDiscount | null);
  let discountPriced = $state(null as DiscountPreview | null);
  let discountProblem = $state(null as string | null);
  let discountBusy = $state(false);
  let discountPin = $state(false);
  let ticketAt = 0;
  let discountRuns = 0;
  const discountKey = $derived(
    JSON.stringify([couponSource, bill.remaining_minor, applied?.code, discountApplied?.discount]),
  );

  function forgetTicket(of: AppliedDiscount | null) {
    if (of?.ticket) void dropElevation(of.ticket);
  }

  async function priceDiscount(chosen: AppliedDiscount, source: CouponSource) {
    const mine = ++discountRuns;
    discountBusy = true;
    const answer = await previewDiscount(chosen.discount, applied?.code, source).finally(() => {
      if (mine === discountRuns) discountBusy = false;
    });
    if (mine !== discountRuns) return;
    if (answer.kind === "ok") {
      discountPriced = answer.preview;
      discountProblem = null;
    } else {
      forgetTicket(chosen);
      discountApplied = null;
      discountPriced = null;
      discountOpen = true;
      discountProblem =
        answer.kind === "refused" ? answer.message : t("hub.settle.discount.unreachable");
    }
  }

  function applyDiscount(chosen: AppliedDiscount) {
    ticketAt = Date.now();
    discountApplied = chosen;
    discountOpen = false;
    discountPin = false;
    discountProblem = null;
  }

  function removeDiscount() {
    discountRuns += 1;
    discountBusy = false;
    forgetTicket(discountApplied);
    discountApplied = null;
    discountPriced = null;
    discountProblem = null;
    discountOpen = false;
    discountPin = false;
  }

  $effect(() => {
    void discountKey;
    untrack(() => {
      if (discountApplied && couponSource) void priceDiscount(discountApplied, couponSource);
    });
  });

  const id = $props.id();
  const exponent = $derived(locale.exponent);
  const total = $derived(
    discountPriced
      ? discountPriced.total_minor
      : applied
        ? applied.total_minor
        : bill.remaining_minor,
  );
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

  const RECEIPT_NAME: Record<ReceiptChoice, MessageKey> = {
    print: "hub.settle.receipt.print",
    none: "hub.settle.receipt.none",
    whatsapp: "hub.settle.receipt.whatsapp",
  };
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

  // A new total (or cash becoming available) starts the split again as one Cash row.
  $effect(() => {
    if (!prefillCash) return;
    const wanted = total;
    const blocked = noCash;
    untrack(() => {
      rows =
        !blocked && wanted > 0
          ? [{ method: "cash", amount: amountText(wanted, exponent), reference: "" }]
          : [];
    });
  });

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

  function onHotkey(event: KeyboardEvent) {
    if (!hotkey || !event.ctrlKey || event.key !== "Enter" || event.isComposing) return;
    event.preventDefault();
    void submit();
  }

  export async function submit() {
    if (busy || couponBusy || discountBusy || !closed || blockedReason || businessBlocked) return;
    attempted = true;
    if (!rowsValid(rows, exponent, wallet)) {
      await tick();
      focusFirstInvalid(root);
      return;
    }
    const staffDiscount = discountApplied;
    const fresh = !!staffDiscount?.ticket && Date.now() - ticketAt < TICKET_FRESH_MS;
    if (staffDiscount && !discountDirect && !fresh) {
      // The approval is used up or too old: ask for the PIN again before settling.
      forgetTicket(staffDiscount);
      discountApplied = { discount: staffDiscount.discount };
      discountPin = true;
      return;
    }
    if (staffDiscount && !discountDirect) discountApplied = { discount: staffDiscount.discount };
    onsettle(
      toPayments(rows, sums.change, exponent),
      receiptChoice ? receipt : undefined,
      toBuyer(businessOffered && businessOn, buyerGstin, buyerName),
      applied?.code,
      staffDiscount ?? undefined,
    );
  }
</script>

<svelte:window onkeydown={onHotkey} />

<section
  class="bar"
  data-component="settle-bar"
  data-testid="settle-bar"
  aria-labelledby="{id}-title"
  bind:this={root}
>
  <h3 class="type-hub-title" id="{id}-title">{t(title)}</h3>
  {#if applied}
    <div class="coupon" data-testid="coupon">
      <p class="type-hub-label coupon-line tabular-nums" data-testid="coupon-line">
        <span>{t("hub.settle.coupon.line", { code: applied.code })}</span>
        <span data-testid="coupon-amount"
          >{t("hub.settle.coupon.amount", { amount: money(applied.discount_minor) })}</span
        >
      </p>
      <button
        type="button"
        class="ui-interactive method type-hub-label"
        data-testid="coupon-remove"
        disabled={busy}
        onclick={removeCoupon}>{t("hub.settle.coupon.remove", { code: applied.code })}</button
      >
    </div>
  {/if}

  {#if discountApplied && discountPriced}
    <div class="coupon" data-testid="discount">
      <p class="type-hub-label coupon-line tabular-nums" data-testid="discount-line">
        <span>{t("hub.settle.discount.line")}</span>
        <span data-testid="discount-amount"
          >{t("hub.settle.coupon.amount", { amount: money(discountPriced.discount_minor) })}</span
        >
      </p>
      <button
        type="button"
        class="ui-interactive method type-hub-label"
        data-testid="discount-remove"
        disabled={busy}
        onclick={removeDiscount}>{t("hub.settle.discount.remove")}</button
      >
    </div>
  {/if}

  <p class="total">
    <span class="type-hub-label">{t("hub.settle.total")}</span>
    {#if couponSource && !applied && !couponOpen}
      <button
        type="button"
        class="ui-interactive method type-hub-label"
        data-testid="coupon-toggle"
        disabled={busy}
        onclick={() => (couponOpen = true)}>{t("hub.settle.coupon.add")}</button
      >
    {/if}
    {#if couponSource && !discountApplied && !discountOpen}
      <button
        type="button"
        class="ui-interactive method type-hub-label"
        data-testid="discount-toggle"
        disabled={busy}
        onclick={() => (discountOpen = true)}>{t("hub.settle.discount.add")}</button
      >
    {/if}
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
  {#if couponSource && couponOpen && !applied}
    <div class="coupon" data-testid="coupon">
      <div class="row coupon-entry">
        <TextField label="hub.settle.coupon" bind:value={couponText} autocomplete="off" />
        <button
          type="button"
          class="ui-interactive method type-hub-label"
          data-testid="coupon-apply"
          disabled={busy || couponBusy || couponText.trim().length === 0}
          onclick={applyCoupon}>{t("hub.settle.coupon.apply")}</button
        >
      </div>
      {#if couponProblem}
        <p class="type-hub-label coupon-problem" role="alert" data-testid="coupon-error">
          <span aria-hidden="true">⚠</span>
          {couponProblem}
        </p>
      {/if}
    </div>
  {/if}

  {#if couponSource && discountOpen && !discountApplied}
    <div class="coupon" data-testid="discount-editor">
      {#if discountProblem}
        <p class="type-hub-label coupon-problem" role="alert" data-testid="discount-error">
          <span aria-hidden="true">⚠</span>
          {discountProblem}
        </p>
      {/if}
      <DiscountEntry
        kind={settleKind(couponSource)}
        direct={discountDirect}
        {exponent}
        onapply={applyDiscount}
        oncancel={() => {
          discountOpen = false;
          discountProblem = null;
        }}
      />
    </div>
  {/if}
  {#if couponSource && discountApplied && discountPin}
    <div class="coupon" data-testid="discount-pin">
      <DiscountEntry
        kind={settleKind(couponSource)}
        direct={false}
        resume={discountApplied.discount}
        {exponent}
        onapply={applyDiscount}
        oncancel={() => (discountPin = false)}
      />
    </div>
  {/if}

  {#if noCash}
    <p class="type-hub-label shift-closed" data-testid="settle-shift-closed">
      <span aria-hidden="true">⚠</span>
      {t("hub.settle.shift_closed")}
      <a class="shift-link" href="/shift">{t("hub.settle.shift_link")}</a>
    </p>
  {/if}

  {@render extra?.()}

  {#if receiptChoice && printerAsked}
    <div class="receipt" data-testid="receipt-choice">
      {#if printable}
        <p class="type-hub-label" data-testid="printer-ready">{t("hub.settle.printer_ready")}</p>
      {:else}
        <p class="type-hub-label" data-testid="printer-missing">
          <span aria-hidden="true">⚠</span>
          {t("hub.settle.no_printer")}
        </p>
      {/if}
      {#if printable || whatsapp}
        <div class="methods" role="group" aria-label={t("hub.settle.receipt")}>
          {#each choices as choice (choice)}
            <button
              type="button"
              class="ui-interactive method type-hub-label"
              data-receipt={choice}
              aria-pressed={receipt === choice}
              disabled={busy}
              onclick={() => (picked = choice)}>{t(RECEIPT_NAME[choice])}</button
            >
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  {#if businessOffered}
    <div class="receipt" data-testid="business-invoice">
      <div class="methods" role="group" aria-label={t("hub.settle.business")}>
        <button
          type="button"
          class="ui-interactive method type-hub-label"
          data-business="toggle"
          aria-pressed={businessOn}
          disabled={busy}
          onclick={() => (businessOn = !businessOn)}>{t("hub.settle.business")}</button
        >
      </div>
      {#if businessOn}
        <div class="row" data-testid="business-fields">
          <TextField
            label="hub.settle.business.gstin"
            bind:value={buyerGstin}
            autocomplete="off"
            error={buyerShown ? (buyerProblems.gstin ?? null) : null}
          />
          <TextField
            label="hub.settle.business.name"
            bind:value={buyerName}
            autocomplete="off"
            error={buyerShown ? (buyerProblems.legal_name ?? null) : null}
          />
        </div>
      {/if}
    </div>
  {/if}

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
    disabled={busy || couponBusy || discountBusy || !closed || !!blockedReason || businessBlocked}
    disabledReason={busy || couponBusy || discountBusy
      ? "hub.sheet.waiting"
      : blockedReason
        ? blockedReason
        : businessBlocked
          ? "hub.settle.business.blocked"
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

  .receipt {
    display: grid;
    gap: var(--space-2);
  }

  .coupon {
    display: grid;
    gap: var(--space-2);
  }

  .coupon-line {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .coupon-entry {
    align-items: end;
  }

  .coupon-problem {
    color: var(--text-field-error-text);
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
