<script lang="ts">
  import { Button, InlinePin, TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { IntentPayload, SessionStaff } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { dropElevation, elevate, hasPermission, newIdempotencyKey, submitIntent } from "./auth";
  import { branchLocale } from "./locale.svelte";
  import { EXPENSE_CATEGORIES, EXPENSE_CATEGORY_LABELS, parseAmount, resizeReceipt } from "./shift";

  /**
   * Deposit and Expense (Story 5.25) on the Shift screen of an open shift. A deposit takes an
   * amount and the slip reference; an expense a category, an amount, who paid (the drawer or the
   * owner) and a receipt photo, resized to at most 200 KB before it is sent. A member without
   * `cash.manage` asks a manager's PIN in place. The expected drawer cash is never shown.
   */
  let {
    staff,
    ontimeout,
    ondone,
  }: {
    staff: SessionStaff | null;
    /** The session ended while the screen was open. */
    ontimeout: () => void;
    /** A deposit or expense was recorded. */
    ondone: () => void;
  } = $props();

  type Mode = "deposit" | "expense";
  const direct = $derived(hasPermission(staff, "cash.manage"));
  const exponent = $derived(branchLocale.current?.exponent ?? 2);

  let mode = $state(null as Mode | null);
  let step = $state("form" as "form" | "pin");
  let amount = $state("");
  let reference = $state("");
  let category = $state("supplies" as (typeof EXPENSE_CATEGORIES)[number]);
  let paidFrom = $state("drawer" as "drawer" | "owner");
  let note = $state("");
  let photo = $state(null as string | null);
  let photoError = $state(false);
  let amountError = $state(null as MessageKey | null);
  let referenceError = $state(null as MessageKey | null);
  let error = $state(null as MessageKey | null);
  let done = $state(null as MessageKey | null);
  let busy = $state(false);
  let attempt: { key: string; body: string } | null = null;
  let root: HTMLElement | undefined = $state();

  async function start(next: Mode) {
    mode = next;
    step = "form";
    amount = "";
    reference = "";
    note = "";
    photo = null;
    photoError = false;
    amountError = referenceError = error = done = null;
    attempt = null;
    await tick();
    root?.querySelector<HTMLInputElement>("form input")?.focus(); // i18n-ignore (a selector)
  }

  function cancel() {
    mode = null;
    error = null;
  }

  function payload(minor: number): IntentPayload {
    if (mode === "deposit") {
      return { kind: "cash.deposit", amount_minor: minor, reference: reference.trim() };
    }
    return {
      kind: "expense.create",
      category,
      amount_minor: minor,
      paid_from: paidFrom,
      receipt_photo: photo ?? undefined,
      note: note.trim() || undefined,
    };
  }

  async function pick(event: Event) {
    const file = (event.currentTarget as HTMLInputElement).files?.[0];
    photo = null;
    photoError = false;
    if (!file) return;
    photo = await resizeReceipt(file);
    photoError = photo === null;
  }

  async function review(event: SubmitEvent) {
    event.preventDefault();
    if (busy || !mode) return;
    amountError = parseAmount(amount, exponent) === null ? "hub.shift.cash.err.amount" : null;
    referenceError =
      mode === "deposit" && !reference.trim() ? "hub.shift.cash.err.reference" : null;
    error = null;
    if (amountError || referenceError) {
      await tick();
      return focusFirstInvalid(root);
    }
    if (direct) void send();
    else step = "pin";
  }

  async function approve(pin: string) {
    const kind = mode === "deposit" ? "cash.deposit" : "expense.create";
    const outcome = await elevate([{ intent_kind: kind }], pin);
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    void send(outcome.ticket);
    return { ok: true } as const;
  }

  async function send(ticket?: string) {
    const minor = parseAmount(amount, exponent);
    if (busy || !mode || minor === null) return;
    busy = true;
    error = null;
    const body = payload(minor);
    const text = JSON.stringify(body);
    // A retry of the same entry reuses its key, so the Hub records it once.
    const key = attempt?.body === text ? attempt.key : newIdempotencyKey();
    attempt = { key, body: text };
    const outcome = await submitIntent(body, key, undefined, { elevationTicket: ticket }).finally(
      () => (busy = false),
    );
    if (outcome.kind === "signed_out") return ontimeout();
    if (outcome.kind === "applied") {
      done = mode === "deposit" ? "hub.shift.cash.deposited" : "hub.shift.cash.spent";
      mode = null;
      attempt = null;
      return ondone();
    }
    if (ticket) void dropElevation(ticket);
    step = "form";
    error =
      outcome.kind === "error"
        ? refusal(outcome.error.code, outcome.error.field)
        : "hub.shift.cash.err.failed";
  }

  function refusal(code: string, field?: string | null): MessageKey {
    if (code === "DRAWER_SHORT") return "hub.shift.cash.err.short";
    if (code === "SHIFT_NOT_OPEN") return "hub.shift.cash.err.shift";
    if (code === "PERMISSION_DENIED") return "hub.refusal.permission";
    if (code === "VALIDATION_FAILED") {
      if (field === "reference") return "hub.shift.cash.err.reference";
      if (field === "amount_minor") return "hub.shift.cash.err.amount";
      if (field === "receipt_photo") return "hub.shift.cash.err.photo";
    }
    return "hub.shift.cash.err.failed";
  }
</script>

<section class="cash" data-testid="cash-moves" aria-labelledby="cash-title" bind:this={root}>
  <h2 class="type-hub-title" id="cash-title">{t("hub.shift.cash.title")}</h2>
  {#if done}
    <p class="type-hub-label" role="status" data-testid="cash-done">{t(done)}</p>
  {/if}

  {#if !mode}
    <div class="actions">
      <Button label="hub.shift.cash.deposit" onclick={() => void start("deposit")} />
      <Button
        label="hub.shift.cash.expense"
        variant="secondary"
        onclick={() => void start("expense")}
      />
    </div>
  {:else if step === "form"}
    <form class="form" onsubmit={review} novalidate data-testid="cash-form">
      <h3 class="type-hub-label">
        {t(mode === "deposit" ? "hub.shift.cash.deposit" : "hub.shift.cash.expense")}
      </h3>
      {#if mode === "expense"}
        <div class="field">
          <label class="type-hub-label" for="cash-category">{t("hub.shift.cash.category")}</label>
          <select id="cash-category" class="ui-interactive control" bind:value={category}>
            {#each EXPENSE_CATEGORIES as c (c)}
              <option value={c}>{t(EXPENSE_CATEGORY_LABELS[c])}</option>
            {/each}
          </select>
        </div>
      {/if}
      <TextField
        label="hub.shift.cash.amount"
        bind:value={amount}
        inputmode="decimal"
        autocomplete="off"
        error={amountError}
      />
      {#if mode === "deposit"}
        <TextField
          label="hub.shift.cash.reference"
          bind:value={reference}
          autocomplete="off"
          error={referenceError}
        />
      {:else}
        <div class="field">
          <label class="type-hub-label" for="cash-paid-from">{t("hub.shift.cash.paid_from")}</label>
          <select id="cash-paid-from" class="ui-interactive control" bind:value={paidFrom}>
            <option value="drawer">{t("hub.shift.cash.paid_from.drawer")}</option>
            <option value="owner">{t("hub.shift.cash.paid_from.owner")}</option>
          </select>
        </div>
        <TextField label="hub.shift.cash.note" bind:value={note} autocomplete="off" />
        <div class="field">
          <label class="type-hub-label" for="cash-photo">{t("hub.shift.cash.photo")}</label>
          <input
            id="cash-photo"
            class="ui-interactive control"
            type="file"
            accept="image/*"
            capture="environment"
            onchange={pick}
            data-testid="cash-photo"
          />
          {#if photoError}
            <p class="type-hub-label refusal" role="alert" data-testid="cash-photo-error">
              {t("hub.shift.cash.err.photo")}
            </p>
          {:else if photo}
            <p class="type-hub-label" data-testid="cash-photo-ready">
              {t("hub.shift.cash.photo_ready")}
            </p>
          {/if}
        </div>
      {/if}
      {#if error}
        <p class="type-hub-body refusal" role="alert" data-testid="cash-error">{t(error)}</p>
      {/if}
      <div class="actions">
        <Button
          label="hub.shift.cash.save"
          type="submit"
          disabled={busy}
          disabledReason="hub.sheet.waiting"
        />
        <Button label="hub.shift.cancel" variant="secondary" onclick={cancel} />
      </div>
    </form>
  {:else}
    <InlinePin
      message={mode === "deposit" ? "hub.shift.cash.pin_deposit" : "hub.shift.cash.pin_expense"}
      onsubmit={approve}
      oncancel={() => (step = "form")}
    />
  {/if}
</section>

<style>
  .cash,
  .form,
  .field {
    display: grid;
    gap: var(--space-3);
  }

  .cash {
    max-inline-size: 32rem;
  }

  h2,
  h3,
  p {
    margin: 0;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .control {
    min-block-size: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface);
    color: var(--text);
    font: inherit;
  }

  .refusal {
    color: var(--warning);
  }
</style>
