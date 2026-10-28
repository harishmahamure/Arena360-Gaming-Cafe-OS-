<script lang="ts">
  import { Button, InlinePin, ReasonPicker, TextField, formatTime, t } from "@arena/ui";
  import type {
    IntentPayload,
    ReasonCode,
    RefundInvoice,
    RefundLineView,
    SessionStaff,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { dropElevation, elevate, hasPermission, newIdempotencyKey, submitIntent } from "./auth";
  import { branchLocale } from "./locale.svelte";
  import { printerReady } from "./printer";
  import {
    LINE_REFUSALS,
    REFUND_METHODS,
    REFUND_METHOD_LABELS,
    REFUND_REASONS,
    REFUND_REASON_LABELS,
    loadRefundInvoice,
    refundRefusal,
    type RefundMethod,
  } from "./refund";

  /**
   * Refund (Story 5.24): the invoice number is typed or scanned, its lines open with what each
   * has had refunded, and the chosen lines go back against a credit note. The reason is picked
   * last; a member without `refund.issue` then asks a manager's PIN in place.
   */
  let {
    staff,
    money,
    ontimeout,
  }: {
    staff: SessionStaff | null;
    money: (minor: number) => string;
    /** The session ended while the screen was open. */
    ontimeout: () => void;
  } = $props();

  const REASON_OPTIONS = REFUND_REASONS.map((value) => ({
    value,
    label: REFUND_REASON_LABELS[value],
  }));
  const direct = $derived(hasPermission(staff, "refund.issue"));

  let no = $state("");
  let invoice = $state(null as RefundInvoice | null);
  let looking = $state(false);
  let lookupError = $state(null as MessageKey | null);
  let qty: Record<string, number> = $state({});
  let ret: Record<string, boolean> = $state({});
  let method = $state("cash" as RefundMethod);
  let reference = $state("");
  let step = $state("form" as "form" | "reason" | "pin");
  let chosen = $state(null as { reason: ReasonCode; note?: string } | null);
  let error = $state(null as MessageKey | null);
  let done = $state(null as string | null);
  let busy = $state(false);
  let attempt: { key: string; body: string } | null = null;
  let root: HTMLElement | undefined = $state();

  const needsReference = $derived(method === "upi" || method === "card");
  const picked = $derived(
    invoice ? invoice.lines.filter((l) => (qty[l.line_id] ?? 0) > 0) : ([] as RefundLineView[]),
  );

  const left = (l: RefundLineView) => (l.refusal ? 0 : Math.max(0, l.qty - l.refunded_qty));

  async function lookup(event?: Event) {
    event?.preventDefault();
    const wanted = no.trim();
    if (!wanted || looking) return;
    looking = true;
    lookupError = null;
    done = null;
    const outcome = await loadRefundInvoice(wanted).finally(() => (looking = false));
    if (outcome.kind === "signed_out") return ontimeout();
    if (outcome.kind === "ok") return open(outcome.invoice);
    invoice = null;
    lookupError =
      outcome.kind === "not_found" ? "hub.refund.err.not_found" : "hub.refund.err.unreachable";
  }

  function open(next: RefundInvoice) {
    invoice = next;
    qty = {};
    ret = {};
    reference = "";
    step = "form";
    chosen = null;
    error = null;
    attempt = null;
    if (!next.has_player && method === "wallet") method = "cash";
  }

  function change(l: RefundLineView, delta: number) {
    const now = qty[l.line_id] ?? 0;
    qty[l.line_id] = Math.min(left(l), Math.max(0, now + delta));
    if (qty[l.line_id] === 0) ret[l.line_id] = false;
    error = null;
  }

  function review() {
    if (!invoice) return;
    if (picked.length === 0) {
      error = "hub.refund.err.pick";
      return;
    }
    if (needsReference && !reference.trim()) {
      error = "hub.refund.err.reference";
      return;
    }
    error = null;
    step = "reason";
  }

  function payload(): IntentPayload {
    const lines = picked.map((l) => ({
      line_id: l.line_id,
      qty: qty[l.line_id] ?? 0,
      return_to_stock: ret[l.line_id] ? true : undefined,
    }));
    return {
      kind: "refund.create",
      invoice_no: invoice?.invoice_no ?? "",
      lines,
      method,
      reference: needsReference ? reference.trim() : undefined,
      note: chosen?.reason === "other" ? chosen.note : undefined,
      receipt: undefined,
    };
  }

  function chooseReason(value: string, note?: string) {
    chosen = { reason: value as ReasonCode, note };
    if (direct) void send();
    else step = "pin";
  }

  async function approve(pin: string) {
    const outcome = await elevate([{ intent_kind: "refund.create" }], pin);
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    void send(outcome.ticket);
    return { ok: true } as const;
  }

  async function backToForm() {
    step = "form";
    await tick();
    root?.querySelector<HTMLElement>('[data-testid="refund-submit"] button')?.focus(); // i18n-ignore (a selector)
  }

  async function send(ticket?: string) {
    if (busy || !invoice || !chosen) return;
    busy = true;
    error = null;
    const printable = await printerReady();
    const body: IntentPayload = {
      ...payload(),
      receipt: printable.kind === "ok" && printable.body.ready ? "print" : undefined,
    } as IntentPayload;
    const text = JSON.stringify(body);
    const key = attempt?.body === text ? attempt.key : newIdempotencyKey();
    attempt = { key, body: text };
    const outcome = await submitIntent(body, key, undefined, {
      elevationTicket: ticket,
      reason: chosen.reason,
    }).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return ontimeout();
    if (outcome.kind === "applied") {
      const wanted = invoice.invoice_no;
      const known = new Set(invoice.refunds.map((r) => r.refund_id));
      const again = await loadRefundInvoice(wanted);
      if (again.kind === "ok") {
        const fresh = again.invoice.refunds.find((r) => !known.has(r.refund_id));
        open(again.invoice);
        done = t("hub.refund.done", {
          number: fresh?.credit_note_no ?? again.invoice.refunds.at(-1)?.credit_note_no ?? "",
        });
      } else {
        open({ ...invoice, lines: [] });
        done = t("hub.refund.done_reload");
      }
      return;
    }
    if (ticket) void dropElevation(ticket);
    step = "form";
    error =
      outcome.kind === "error"
        ? refundRefusal(outcome.error.code, outcome.error.field)
        : "hub.refund.err.unreachable";
  }
</script>

<section class="refund" data-testid="refund" aria-labelledby="refund-title" bind:this={root}>
  <h2 class="type-hub-title" id="refund-title">{t("hub.refund.title")}</h2>
  <form class="find" onsubmit={lookup}>
    <TextField label="hub.refund.invoice_label" bind:value={no} autocomplete="off" />
    <Button label="hub.refund.find" type="submit" />
  </form>
  {#if lookupError}
    <p class="type-hub-body refusal" role="alert" data-testid="refund-lookup-error">
      {t(lookupError)}
    </p>
  {/if}
  {#if done}
    <p class="type-hub-body" role="status" data-testid="refund-done">{done}</p>
  {/if}

  {#if invoice}
    <div class="invoice" data-testid="refund-invoice">
      <p class="type-hub-label" data-testid="refund-invoice-no">
        {t("hub.refund.invoice", { number: invoice.invoice_no })} · {formatTime(
          invoice.settled_at,
          branchLocale.current?.tz,
        )}
      </p>
      <p class="type-hub-label tabular-nums" data-testid="refund-totals">
        {t("hub.refund.totals", {
          paid: money(invoice.paid_minor),
          refunded: money(invoice.refunded_minor),
        })}
      </p>
      <ul class="lines" aria-label={t("hub.refund.lines")}>
        {#each invoice.lines as line (line.line_id)}
          {@const room = left(line)}
          <li class="line" data-testid="refund-line" data-line-id={line.line_id}>
            <div class="what">
              <span class="type-hub-body">{line.label}</span>
              <span class="type-hub-label tabular-nums"
                >{t("hub.refund.line.sold", {
                  qty: line.qty,
                  refunded: line.refunded_qty,
                  amount: money(line.amount_minor),
                })}</span
              >
              {#if line.refusal}
                <span class="type-hub-label refusal" data-testid="refund-line-refusal"
                  >{t(LINE_REFUSALS[line.refusal] ?? "hub.refund.line.plan_not_refundable")}</span
                >
              {:else if room === 0}
                <span class="type-hub-label">{t("hub.refund.line.all_refunded")}</span>
              {/if}
            </div>
            {#if room > 0}
              {#if line.kind === "product"}
                <div class="stepper" role="group" aria-label={line.label}>
                  <button
                    type="button"
                    class="ui-interactive step type-hub-label"
                    aria-label={t("hub.refund.less", { name: line.label })}
                    disabled={(qty[line.line_id] ?? 0) === 0}
                    onclick={() => change(line, -1)}>−</button
                  >
                  <span class="type-hub-numeric tabular-nums" data-testid="refund-qty"
                    >{qty[line.line_id] ?? 0}</span
                  >
                  <button
                    type="button"
                    class="ui-interactive step type-hub-label"
                    aria-label={t("hub.refund.more", { name: line.label })}
                    disabled={(qty[line.line_id] ?? 0) >= room}
                    onclick={() => change(line, 1)}>+</button
                  >
                </div>
              {:else}
                <label class="check type-hub-label">
                  <input
                    type="checkbox"
                    checked={(qty[line.line_id] ?? 0) > 0}
                    onchange={(e) => {
                      qty[line.line_id] = e.currentTarget.checked ? 1 : 0;
                      error = null;
                    }}
                  />
                  {t("hub.refund.line.whole")}
                </label>
              {/if}
              {#if line.tracked && (qty[line.line_id] ?? 0) > 0}
                <label class="check type-hub-label">
                  <input type="checkbox" bind:checked={ret[line.line_id]} />
                  {t("hub.refund.line.return_stock")}
                </label>
              {/if}
            {/if}
          </li>
        {/each}
      </ul>

      {#if step === "form"}
        <div class="method">
          <label class="type-hub-label" for="refund-method">{t("hub.refund.method")}</label>
          <select
            id="refund-method"
            class="ui-interactive control"
            bind:value={method}
            onchange={() => (error = null)}
          >
            {#each REFUND_METHODS as m (m)}
              {#if m !== "wallet" || invoice.has_player}
                <option value={m}>{t(REFUND_METHOD_LABELS[m])}</option>
              {/if}
            {/each}
          </select>
          {#if needsReference}
            <TextField
              label={method === "upi" ? "hub.refund.reference_upi" : "hub.refund.reference_card"}
              bind:value={reference}
              autocomplete="off"
            />
          {/if}
        </div>
        {#if error}
          <p class="type-hub-body refusal" role="alert" data-testid="refund-error">{t(error)}</p>
        {/if}
        <div class="actions" data-testid="refund-submit">
          <Button
            label="hub.refund.submit"
            disabled={busy}
            disabledReason="hub.sheet.waiting"
            onclick={review}
          />
        </div>
      {:else if step === "reason"}
        <ReasonPicker
          options={REASON_OPTIONS}
          legend="hub.refund.why"
          noteFor="other"
          noteLabel="hub.refund.note"
          onconfirm={chooseReason}
          oncancel={() => void backToForm()}
        />
      {:else}
        <InlinePin message="hub.refund.pin" onsubmit={approve} oncancel={() => void backToForm()} />
      {/if}

      {#if invoice.refunds.length > 0}
        <h3 class="type-hub-label">{t("hub.refund.issued")}</h3>
        <ul class="notes" data-testid="refund-notes">
          {#each invoice.refunds as r (r.refund_id)}
            <li class="type-hub-label tabular-nums">
              {t("hub.refund.note_line", {
                number: r.credit_note_no,
                amount: money(r.amount_minor),
              })}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</section>

<style>
  .refund,
  .invoice {
    display: grid;
    gap: var(--space-3);
  }

  .find,
  .method,
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: var(--space-3);
  }

  .lines,
  .notes {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding-block-end: var(--space-3);
    border-block-end: 1px solid var(--line);
  }

  .what {
    display: grid;
    gap: var(--space-1);
  }

  .stepper,
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .step {
    --ui-bg: var(--button-secondary-background);
    min-block-size: var(--space-touch-min);
    min-inline-size: var(--space-touch-min);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    color: var(--button-secondary-foreground);
    font-weight: var(--type-hub-title-weight);
    cursor: pointer;
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
