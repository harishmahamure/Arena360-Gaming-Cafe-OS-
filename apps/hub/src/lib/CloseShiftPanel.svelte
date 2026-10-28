<script lang="ts">
  import { Button, InlinePin, TextField, t } from "@arena/ui";
  import type { IntentPayload, SessionStaff, ShiftCount } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { dropElevation, elevate, hasPermission, newIdempotencyKey, submitIntent } from "./auth";
  import DenominationCounter from "./DenominationCounter.svelte";
  import { branchLocale } from "./locale.svelte";
  import { VARIANCE_REASONS, VARIANCE_REASON_LABELS, denominationsFor, parseAmount } from "./shift";
  import { revealCount } from "./shift-count";
  import VarianceReadout from "./VarianceReadout.svelte";

  /**
   * Close shift (Story 5.26). Staff count the drawer first; only then does the Hub reveal the
   * expected cash, and the readout shows the signed difference. Within the branch threshold the
   * shift closes at once. Over it, the close needs a reason and a manager's inline PIN (unless the
   * member holds `shift.override_variance`). An optional deposit at close is recorded after the
   * count and lowers the next shift's float.
   */
  let {
    staff,
    ontimeout,
    ondone,
    oncancel,
  }: {
    staff: SessionStaff | null;
    /** The session ended while the screen was open. */
    ontimeout: () => void;
    /** The shift closed. */
    ondone: () => void;
    /** Staff backed out. */
    oncancel: () => void;
  } = $props();

  const locale = $derived(branchLocale.current);
  const exponent = $derived(locale?.exponent ?? 2);
  const direct = $derived(hasPermission(staff, "shift.override_variance"));

  let total = $state(null as number | null);
  let countError = $state(null as MessageKey | null);
  let reveal = $state(null as ShiftCount | null);
  let withDeposit = $state(false);
  let depositText = $state("");
  let reference = $state("");
  let depositError = $state(null as MessageKey | null);
  let referenceError = $state(null as MessageKey | null);
  let reason = $state("" as (typeof VARIANCE_REASONS)[number] | "");
  let note = $state("");
  let reasonError = $state(null as MessageKey | null);
  let noteError = $state(null as MessageKey | null);
  let error = $state(null as MessageKey | null);
  let step = $state("count" as "count" | "pin");
  let busy = $state(false);
  let attempt: { key: string; body: string } | null = null;
  let root: HTMLElement | undefined = $state();

  // Changing the count after it was confirmed hides the reveal again: the Hub only ever shows the
  // expected cash for the count on screen.
  $effect(() => {
    if (reveal && total !== reveal.counted_minor) reveal = null;
  });

  async function confirmCount(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    if (total === null) {
      countError = "hub.shift.close.err.count";
      await tick();
      return root?.querySelector<HTMLElement>("[aria-invalid='true']")?.focus();
    }
    countError = error = null;
    busy = true;
    const outcome = await revealCount(total).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return ontimeout();
    if (outcome.kind === "ok") {
      reveal = outcome.count;
      return;
    }
    error =
      outcome.kind === "error" && outcome.error
        ? refusal(outcome.error.code, outcome.error.field)
        : "hub.shift.close.err.failed";
  }

  function body(counted: number): IntentPayload | null {
    if (!reveal) return null;
    depositError = referenceError = reasonError = noteError = null;
    let deposit: { amount_minor: number; reference: string } | undefined;
    if (withDeposit) {
      const amount = parseAmount(depositText, exponent);
      if (amount === null || amount > counted) depositError = "hub.shift.close.err.deposit";
      if (!reference.trim()) referenceError = "hub.shift.cash.err.reference";
      if (depositError || referenceError || amount === null) return null;
      deposit = { amount_minor: amount, reference: reference.trim() };
    }
    if (reveal.over_threshold) {
      if (!reason) reasonError = "hub.shift.close.err.reason";
      else if (reason === "other" && !note.trim()) noteError = "hub.shift.close.err.note";
      if (reasonError || noteError || !reason) return null;
    }
    return {
      kind: "shift.close",
      counted_minor: counted,
      deposit,
      reason: reveal.over_threshold ? (reason as (typeof VARIANCE_REASONS)[number]) : undefined,
      note: reveal.over_threshold && reason === "other" ? note.trim() : undefined,
    };
  }

  async function close(event: SubmitEvent) {
    event.preventDefault();
    if (busy || !reveal) return;
    error = null;
    if (!body(reveal.counted_minor)) {
      await tick();
      return root?.querySelector<HTMLElement>("[aria-invalid='true']")?.focus();
    }
    if (reveal.over_threshold && !direct) step = "pin";
    else void send();
  }

  async function approve(pin: string) {
    const outcome = await elevate([{ intent_kind: "shift.close" }], pin);
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind === "signed_out") {
      ontimeout();
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    void send(outcome.ticket);
    return { ok: true } as const;
  }

  async function send(ticket?: string) {
    const payload = reveal ? body(reveal.counted_minor) : null;
    if (busy || !payload) return;
    busy = true;
    error = null;
    const text = JSON.stringify(payload);
    // A retry of the same close reuses its key, so the Hub closes the shift once.
    const key = attempt?.body === text ? attempt.key : newIdempotencyKey();
    attempt = { key, body: text };
    const outcome = await submitIntent(payload, key, undefined, {
      elevationTicket: ticket,
    }).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return ontimeout();
    if (outcome.kind === "applied") {
      attempt = null;
      return ondone();
    }
    if (ticket) void dropElevation(ticket);
    step = "count";
    // The drawer moved since the reveal, so the difference on screen is stale: count again to see
    // the current one (and the reason and PIN it may now need).
    if (outcome.kind === "error" && outcome.error.code === "VARIANCE_OVER_THRESHOLD") reveal = null;
    error =
      outcome.kind === "error"
        ? refusal(outcome.error.code, outcome.error.field)
        : "hub.shift.close.err.failed";
  }

  function refusal(code: string, field?: string | null): MessageKey {
    if (code === "VARIANCE_OVER_THRESHOLD") return "hub.shift.close.err.over";
    if (code === "SHIFT_NOT_OPEN") return "hub.shift.close.err.shift";
    if (code === "PERMISSION_DENIED") return "hub.refusal.permission";
    if (code === "VALIDATION_FAILED") {
      if (field === "reason") return "hub.shift.close.err.reason";
      if (field === "note") return "hub.shift.close.err.note";
      if (field === "deposit") return "hub.shift.close.err.deposit";
      if (field === "reference") return "hub.shift.cash.err.reference";
      if (field === "counted_minor") return "hub.shift.close.err.count";
    }
    return "hub.shift.close.err.failed";
  }
</script>

{#if locale}
  <section class="close" data-testid="close-shift" aria-labelledby="close-title" bind:this={root}>
    <h2 class="type-hub-title" id="close-title">{t("hub.shift.close.title")}</h2>
    {#if step === "count"}
      <form class="form" onsubmit={reveal ? close : confirmCount} novalidate>
        <DenominationCounter
          denominations={denominationsFor(locale.currency)}
          {locale}
          bind:total
          error={countError}
        />
        <VarianceReadout {reveal} {locale} />
        {#if !reveal}
          <div class="actions">
            <Button
              label="hub.shift.close.confirm_count"
              type="submit"
              disabled={busy}
              disabledReason="hub.sheet.waiting"
            />
            <Button label="hub.shift.cancel" variant="secondary" onclick={oncancel} />
          </div>
        {:else}
          <label class="toggle type-hub-body">
            <input type="checkbox" bind:checked={withDeposit} data-testid="close-deposit-toggle" />
            <span>{t("hub.shift.close.deposit")}</span>
          </label>
          {#if withDeposit}
            <TextField
              label="hub.shift.cash.amount"
              bind:value={depositText}
              inputmode="decimal"
              autocomplete="off"
              error={depositError}
            />
            <TextField
              label="hub.shift.cash.reference"
              bind:value={reference}
              autocomplete="off"
              error={referenceError}
            />
          {/if}
          {#if reveal.over_threshold}
            <div class="field">
              <label class="type-hub-label" for="close-reason">{t("hub.shift.close.reason")}</label>
              <select
                id="close-reason"
                class="ui-interactive control"
                bind:value={reason}
                aria-invalid={reasonError ? "true" : undefined}
              >
                <option value="">{t("hub.shift.close.reason.choose")}</option>
                {#each VARIANCE_REASONS as r (r)}
                  <option value={r}>{t(VARIANCE_REASON_LABELS[r])}</option>
                {/each}
              </select>
              {#if reasonError}
                <p class="type-hub-label refusal" role="alert">{t(reasonError)}</p>
              {/if}
            </div>
            {#if reason === "other"}
              <TextField
                label="hub.shift.close.note"
                bind:value={note}
                autocomplete="off"
                error={noteError}
              />
            {/if}
          {/if}
          <div class="actions">
            <Button
              label="hub.shift.close.submit"
              type="submit"
              disabled={busy}
              disabledReason="hub.sheet.waiting"
            />
            <Button label="hub.shift.cancel" variant="secondary" onclick={oncancel} />
          </div>
        {/if}
        {#if error}
          <p class="type-hub-body refusal" role="alert" data-testid="close-error">
            <span aria-hidden="true">⚠</span>
            {t(error)}
          </p>
        {/if}
      </form>
    {:else}
      <InlinePin
        message="hub.shift.close.pin"
        onsubmit={approve}
        oncancel={() => (step = "count")}
      />
    {/if}
  </section>
{/if}

<style>
  .close,
  .form,
  .field {
    display: grid;
    gap: var(--space-3);
  }

  .close {
    max-inline-size: 36rem;
  }

  h2,
  p {
    margin: 0;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .toggle {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
    cursor: pointer;
  }

  .toggle input {
    flex: none;
    width: var(--space-5);
    height: var(--space-5);
    margin: 0;
    accent-color: var(--accent);
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
