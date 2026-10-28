<script lang="ts">
  import { Button, formatDateTime, formatMoney, formatTime, t } from "@arena/ui";
  import type { ReportAmount, ZReport } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { SESSION_DEPENDENCY, hasPermission } from "../../../../lib/auth";
  import { branchLocale } from "../../../../lib/locale.svelte";
  import { sessionStaff } from "../../../../lib/operating";
  import ExportPanel from "../../../../lib/ExportPanel.svelte";
  import { loadZReport, reprintZReport } from "../../../../lib/reports";
  import ScrollRegion from "../../../../lib/ScrollRegion.svelte";
  import { EXPENSE_CATEGORY_LABELS, VARIANCE_REASON_LABELS } from "../../../../lib/shift";

  const METHOD: Record<string, MessageKey> = {
    cash: "hub.zreport.method.cash",
    upi: "hub.zreport.method.upi",
    card: "hub.zreport.method.card",
    wallet: "hub.zreport.method.wallet",
    credit: "hub.zreport.method.credit",
  };
  const TYPE: Record<string, MessageKey> = {
    play_time: "hub.reports.type.play_time",
    products: "hub.reports.type.products",
    plans: "hub.reports.type.plans",
    discounts: "hub.reports.type.discounts",
    tax_added: "hub.reports.type.tax_added",
  };
  const RECEIVED: Record<string, MessageKey> = {
    topup: "hub.reports.received.topup",
    credit_collect: "hub.reports.received.credit_collect",
  };
  /** Waste and void reasons share the reason catalogue; an unknown reason shows as it is. */
  const REASON: Record<string, MessageKey> = {
    spoiled: "reason.spoiled",
    broken: "reason.broken",
    free: "reason.free",
    cancelled_order: "reason.cancelled_order",
    mistake: "reason.mistake",
    changed_mind: "reason.changed_mind",
    other: "reason.other",
  };

  const locale = $derived(branchLocale.current);
  const allowed = $derived(hasPermission(sessionStaff(page.data), "report.view"));
  const shiftId = $derived(page.params.id ?? "");

  let view = $state<ZReport | null>(null);
  let problem: MessageKey | null = $state(null);
  let loading = $state(false);
  let busy = $state(false);
  let status: MessageKey | null = $state(null);
  let refusal: MessageKey | null = $state(null);

  async function load() {
    loading = true;
    const outcome = await loadZReport(shiftId);
    loading = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    problem =
      outcome.kind === "ok"
        ? null
        : outcome.kind === "not_found"
          ? "hub.zreport.not_found"
          : outcome.kind === "open"
            ? "hub.zreport.still_open"
            : outcome.kind === "denied"
              ? "hub.reports.no_access"
              : "hub.reports.failed";
    if (outcome.kind === "ok") view = outcome.view;
  }

  $effect(() => {
    void shiftId;
    view = null;
    status = null;
    refusal = null;
    if (allowed) void load();
  });

  async function reprint() {
    if (busy) return;
    busy = true;
    status = null;
    refusal = null;
    const outcome = await reprintZReport(shiftId).finally(() => (busy = false));
    if (outcome === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome === "queued") status = "hub.zreport.reprint_queued";
    else
      refusal =
        outcome === "refused" ? "hub.zreport.reprint_refused" : "hub.zreport.reprint_failed";
  }

  const label = (map: Record<string, MessageKey>, key: string) => (key in map ? t(map[key]) : key);
  const count = (rows: ReportAmount[]) => rows.filter((r) => r.count > 0);
  const signed = (minor: number) =>
    minor > 0 ? `+${formatMoney(minor, locale!)}` : formatMoney(minor, locale!); // i18n-ignore
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.zreport.title") })}</title>
</svelte:head>

<section class="report" data-testid="z-report-screen">
  <h1 class="type-hub-numeric">{t("hub.zreport.title")}</h1>
  <p class="type-hub-label"><a href="/reports/shifts">{t("hub.zreport.back")}</a></p>

  {#if !allowed}
    <p class="type-hub-body note" role="status" data-testid="reports-denied">
      {t("hub.reports.no_access")}
    </p>
  {:else}
    <div role="status">
      {#if problem}
        <p class="type-hub-label warn" data-testid="report-problem">
          <span aria-hidden="true">⚠</span>
          {t(problem)}
        </p>
      {:else if loading}
        <p class="type-hub-label note">{t("hub.reports.loading")}</p>
      {/if}
    </div>

    {#if view && locale}
      <div class="actions">
        <Button
          label="hub.zreport.reprint"
          variant="secondary"
          onclick={() => void reprint()}
          disabled={busy}
          disabledReason="hub.sheet.waiting"
        />
      </div>
      <div role="status">
        {#if status}
          <p class="type-hub-label" data-testid="z-reprint-status">{t(status)}</p>
        {/if}
      </div>
      {#if refusal}
        <p class="type-hub-label warn" role="alert" data-testid="z-reprint-refusal">
          <span aria-hidden="true">⚠</span>
          {t(refusal)}
        </p>
      {/if}
      {#if view.rebuilt}
        <p class="type-hub-label note" data-testid="z-rebuilt">{t("hub.zreport.rebuilt")}</p>
      {/if}

      <dl class="facts type-hub-body" data-testid="z-shift">
        <div>
          <dt class="type-hub-label">{t("hub.reports.col.opened")}</dt>
          <dd class="tabular-nums" data-testid="z-opened">
            {formatDateTime(view.opened_at, locale.tz)}
          </dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.reports.col.closed")}</dt>
          <dd class="tabular-nums" data-testid="z-closed">
            {formatDateTime(view.closed_at, locale.tz)}
          </dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.reports.col.opened_by")}</dt>
          <dd>{view.opened_by}</dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.reports.col.closed_by")}</dt>
          <dd>{view.closed_by}</dd>
        </div>
      </dl>

      <h2 class="type-hub-body">{t("hub.reports.by_type")}</h2>
      <ScrollRegion label="hub.reports.by_type">
        <table class="type-hub-body" data-testid="z-sales">
          <caption class="sr-only">{t("hub.reports.by_type")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("hub.reports.col.type")}</th>
              <th scope="col" class="num">{t("hub.reports.col.amount")}</th>
            </tr>
          </thead>
          <tbody>
            {#each view.by_type as row (row.key)}
              <tr data-testid="z-type-{row.key}">
                <th scope="row">{label(TYPE, row.key)}</th>
                <td class="num tabular-nums">{formatMoney(row.amount_minor, locale)}</td>
              </tr>
            {/each}
            <tr class="total" data-testid="z-sales-total">
              <th scope="row">{t("hub.zreport.total_sales")}</th>
              <td class="num tabular-nums">{formatMoney(view.sales_minor, locale)}</td>
            </tr>
          </tbody>
        </table>
      </ScrollRegion>
      <p class="type-hub-label note">
        {t("hub.zreport.bills_tax", {
          bills: String(view.orders),
          tax: formatMoney(view.tax_minor, locale),
        })}
      </p>

      <h2 class="type-hub-body">{t("hub.zreport.payments")}</h2>
      {#if view.payments.length === 0}
        <p class="type-hub-label note" data-testid="z-payments-none">{t("hub.zreport.none")}</p>
      {:else}
        <ScrollRegion label="hub.zreport.payments">
          <table class="type-hub-body" data-testid="z-payments">
            <caption class="sr-only">{t("hub.zreport.payments")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("hub.zreport.col.method")}</th>
                <th scope="col" class="num">{t("hub.zreport.col.count")}</th>
                <th scope="col" class="num">{t("hub.reports.col.amount")}</th>
              </tr>
            </thead>
            <tbody>
              {#each view.payments as p (p.key)}
                <tr data-testid="z-payment-{p.key}">
                  <th scope="row">{label(METHOD, p.key)}</th>
                  <td class="num tabular-nums">{p.count}</td>
                  <td class="num tabular-nums">{formatMoney(p.amount_minor, locale)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </ScrollRegion>
      {/if}
      {#if count(view.received).length > 0}
        <p class="type-hub-label note">{t("hub.reports.received_title")}</p>
        <ul class="plain type-hub-body" data-testid="z-received">
          {#each count(view.received) as r (r.key)}
            <li>
              {label(RECEIVED, r.key)}: {formatMoney(r.amount_minor, locale)}
              ({r.count})
            </li>
          {/each}
        </ul>
      {/if}

      {#if view.references.length > 0}
        <h2 class="type-hub-body">{t("hub.zreport.references")}</h2>
        <ScrollRegion label="hub.zreport.references">
          {#each view.references as group (group.method)}
            <table class="type-hub-body" data-testid="z-references-{group.method}">
              <caption>{label(METHOD, group.method)}</caption>
              <thead>
                <tr>
                  <th scope="col">{t("hub.zreport.col.time")}</th>
                  <th scope="col">{t("hub.zreport.col.invoice")}</th>
                  <th scope="col" class="num">{t("hub.reports.col.amount")}</th>
                  <th scope="col">{t("hub.zreport.col.reference")}</th>
                </tr>
              </thead>
              <tbody>
                {#each group.rows as row, i (i)}
                  <tr data-testid="z-reference-row">
                    <th scope="row" class="tabular-nums">{formatTime(row.at, locale.tz)}</th>
                    <td>{row.invoice_no}</td>
                    <td class="num tabular-nums">{formatMoney(row.amount_minor, locale)}</td>
                    <td>{row.reference}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/each}
        </ScrollRegion>
      {/if}

      <h2 class="type-hub-body">{t("hub.reports.refunds")}</h2>
      {#if view.refunds.length === 0}
        <p class="type-hub-label note" data-testid="z-refunds-none">{t("hub.zreport.none")}</p>
      {:else}
        <ScrollRegion label="hub.reports.refunds">
          <table class="type-hub-body" data-testid="z-refunds">
            <caption class="sr-only">{t("hub.reports.refunds")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("hub.zreport.col.credit_note")}</th>
                <th scope="col">{t("hub.zreport.col.method")}</th>
                <th scope="col" class="num">{t("hub.reports.col.amount")}</th>
              </tr>
            </thead>
            <tbody>
              {#each view.refunds as r, i (i)}
                <tr data-testid="z-refund-row">
                  <th scope="row">{r.credit_note_no}</th>
                  <td>{r.method ? label(METHOD, r.method) : ""}</td>
                  <td class="num tabular-nums">{formatMoney(r.amount_minor, locale)}</td>
                </tr>
              {/each}
              <tr class="total" data-testid="z-refunds-total">
                <th scope="row" colspan="2">{t("hub.zreport.total_refunds")}</th>
                <td class="num tabular-nums">{formatMoney(view.refunds_minor, locale)}</td>
              </tr>
            </tbody>
          </table>
        </ScrollRegion>
      {/if}

      <h2 class="type-hub-body">{t("hub.zreport.waste")}</h2>
      {#if view.waste.length === 0}
        <p class="type-hub-label note" data-testid="z-waste-none">{t("hub.zreport.none")}</p>
      {:else}
        <ul class="plain type-hub-body" data-testid="z-waste">
          {#each view.waste as w (w.reason)}
            <li data-testid="z-waste-row">
              {label(REASON, w.reason)} × {w.qty}: {formatMoney(w.value_minor, locale)}
            </li>
          {/each}
        </ul>
      {/if}

      <h2 class="type-hub-body">{t("hub.zreport.expenses")}</h2>
      {#if view.expenses.length === 0}
        <p class="type-hub-label note" data-testid="z-expenses-none">{t("hub.zreport.none")}</p>
      {:else}
        <ul class="plain type-hub-body" data-testid="z-expenses">
          {#each view.expenses as e, i (i)}
            <li data-testid="z-expense-row">
              {e.category in EXPENSE_CATEGORY_LABELS
                ? t(EXPENSE_CATEGORY_LABELS[e.category as keyof typeof EXPENSE_CATEGORY_LABELS])
                : e.category}
              ({e.paid_from === "owner"
                ? t("hub.zreport.paid_owner")
                : t("hub.zreport.paid_drawer")}):
              {formatMoney(e.amount_minor, locale)}{e.note ? ` · ${e.note}` : ""}
            </li>
          {/each}
        </ul>
      {/if}

      <h2 class="type-hub-body">{t("hub.zreport.deposits")}</h2>
      {#if view.deposits.length === 0}
        <p class="type-hub-label note" data-testid="z-deposits-none">{t("hub.zreport.none")}</p>
      {:else}
        <ul class="plain type-hub-body" data-testid="z-deposits">
          {#each view.deposits as d, i (i)}
            <li data-testid="z-deposit-row">
              {d.reference}: {formatMoney(d.amount_minor, locale)}
            </li>
          {/each}
        </ul>
      {/if}

      <h2 class="type-hub-body">{t("hub.zreport.voids")}</h2>
      {#if view.voids.length === 0}
        <p class="type-hub-label note" data-testid="z-voids-none">{t("hub.zreport.none")}</p>
      {:else}
        <ul class="plain type-hub-body" data-testid="z-voids">
          {#each view.voids as v (v.reason)}
            <li data-testid="z-void-row">
              {label(REASON, v.reason)} × {v.count}: {formatMoney(v.amount_minor, locale)}
            </li>
          {/each}
        </ul>
      {/if}

      <p class="type-hub-body" data-testid="z-drawer-opens">
        {t("hub.zreport.drawer_opens", { count: String(view.drawer_opens) })}
      </p>

      <h2 class="type-hub-body">{t("hub.zreport.cash_count")}</h2>
      <dl class="facts type-hub-body" data-testid="z-cash">
        <div>
          <dt class="type-hub-label">{t("hub.zreport.float")}</dt>
          <dd class="tabular-nums" data-testid="z-float">
            {formatMoney(view.float_minor, locale)}
          </dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.shift.close.expected")}</dt>
          <dd class="tabular-nums" data-testid="z-expected">
            {formatMoney(view.expected_minor, locale)}
          </dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.shift.close.counted")}</dt>
          <dd class="tabular-nums" data-testid="z-counted">
            {formatMoney(view.counted_minor, locale)}
          </dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.shift.close.difference")}</dt>
          <dd class="tabular-nums" data-testid="z-variance">{signed(view.variance_minor)}</dd>
        </div>
        {#if view.variance_reason}
          <div>
            <dt class="type-hub-label">{t("hub.zreport.reason")}</dt>
            <dd data-testid="z-reason">
              {view.variance_reason in VARIANCE_REASON_LABELS
                ? t(
                    VARIANCE_REASON_LABELS[
                      view.variance_reason as keyof typeof VARIANCE_REASON_LABELS
                    ],
                  )
                : view.variance_reason}
              {view.variance_note ? ` · ${view.variance_note}` : ""}
            </dd>
          </div>
        {:else if view.variance_note}
          <div>
            <dt class="type-hub-label">{t("hub.zreport.reason")}</dt>
            <dd data-testid="z-reason">{view.variance_note}</dd>
          </div>
        {/if}
        {#if view.approver}
          <div>
            <dt class="type-hub-label">{t("hub.zreport.approver")}</dt>
            <dd data-testid="z-approver">{view.approver}</dd>
          </div>
        {/if}
      </dl>

      <ExportPanel report="z-report" selection={{ shift_id: view.shift_id }} />
    {/if}
  {/if}
</section>

<style>
  .report {
    display: grid;
    gap: var(--space-3);
  }

  h1,
  h2,
  p,
  dl,
  dd {
    margin: 0;
  }

  .note {
    color: var(--text-muted);
  }

  .warn {
    color: var(--text-field-error-text);
  }

  .actions {
    display: flex;
    gap: var(--space-3);
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-5);
  }

  .facts dt {
    color: var(--text-muted);
  }

  .plain {
    display: grid;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  table {
    inline-size: 100%;
    max-inline-size: 48rem;
    border-collapse: collapse;
  }

  table + table {
    margin-block-start: var(--space-3);
  }

  caption {
    padding-block-end: var(--space-1);
    text-align: start;
    font-weight: 600;
  }

  th,
  td {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border-strong);
    text-align: start;
    vertical-align: middle;
    font-weight: inherit;
  }

  .total th,
  .total td {
    font-weight: 600;
  }

  .num {
    text-align: end;
  }

  .sr-only {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
