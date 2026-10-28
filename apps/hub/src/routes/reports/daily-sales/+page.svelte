<script lang="ts">
  import { formatMoney, t } from "@arena/ui";
  import type { DailySales } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { SESSION_DEPENDENCY, hasPermission } from "../../../lib/auth";
  import { branchLocale } from "../../../lib/locale.svelte";
  import { sessionStaff } from "../../../lib/operating";
  import ExportPanel from "../../../lib/ExportPanel.svelte";
  import ReportRange from "../../../lib/ReportRange.svelte";
  import ScrollRegion from "../../../lib/ScrollRegion.svelte";
  import { businessDayNow, hourText, loadDailySales, percentOf } from "../../../lib/reports";

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

  const locale = $derived(branchLocale.current);
  const allowed = $derived(hasPermission(sessionStaff(page.data), "report.view"));

  let from = $state("");
  let to = $state("");
  let view = $state<DailySales | null>(null);
  let problem: MessageKey | null = $state(null);
  let loading = $state(false);
  let turn = 0;

  async function load() {
    const mine = ++turn;
    loading = true;
    const outcome = await loadDailySales(from, to);
    if (mine !== turn) return;
    loading = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    problem =
      outcome.kind === "ok"
        ? null
        : outcome.kind === "invalid"
          ? "hub.reports.range_invalid"
          : outcome.kind === "denied"
            ? "hub.reports.no_access"
            : "hub.reports.failed";
    if (outcome.kind === "ok") view = outcome.view;
  }

  // Opens on today once the branch locale (its time zone and day start) and the session are in.
  $effect(() => {
    if (locale && allowed && !from) {
      from = to = businessDayNow(locale, Date.now());
      untrack(() => void load());
    }
  });

  const hours = $derived((view?.by_hour ?? []).filter((h) => h.count > 0));
  const peak = $derived(Math.max(0, ...hours.map((h) => h.amount_minor)));
  const groupName = (name: string) => name || t("hub.reports.counter");
  const categoryName = (name: string) => name || t("hub.reports.uncategorised");
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.reports.daily_sales") })}</title>
</svelte:head>

<section class="report" data-testid="daily-sales-screen">
  <h1 class="type-hub-numeric">{t("hub.reports.daily_sales")}</h1>
  <p class="type-hub-label"><a href="/reports">{t("hub.reports.back")}</a></p>

  {#if !allowed}
    <p class="type-hub-body note" role="status" data-testid="reports-denied">
      {t("hub.reports.no_access")}
    </p>
  {:else}
    <ReportRange bind:from bind:to onchange={() => void load()} />

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
      <dl class="totals" data-testid="sales-totals">
        <div>
          <dt class="type-hub-label">{t("hub.reports.sales")}</dt>
          <dd class="type-hub-numeric" data-testid="sales-gross">
            {formatMoney(view.sales_minor, locale)}
          </dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.reports.refunds")}</dt>
          <dd class="type-hub-numeric" data-testid="sales-refunds">
            {formatMoney(-view.refunds_minor, locale)}
          </dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.reports.net")}</dt>
          <dd class="type-hub-numeric" data-testid="sales-net">
            {formatMoney(view.net_minor, locale)}
          </dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.reports.tax_inside")}</dt>
          <dd class="type-hub-body" data-testid="sales-tax">
            {formatMoney(view.tax_minor, locale)}
          </dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.reports.bills")}</dt>
          <dd class="type-hub-body" data-testid="sales-orders">{view.orders}</dd>
        </div>
      </dl>

      {#if view.orders === 0}
        <p class="type-hub-body note" data-testid="sales-empty">{t("hub.reports.no_sales")}</p>
      {/if}

      <h2 class="type-hub-body">{t("hub.reports.by_type")}</h2>
      <ScrollRegion label="hub.reports.by_type">
        <table class="type-hub-body" data-testid="sales-by-type">
          <caption class="sr-only">{t("hub.reports.by_type")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("hub.reports.col.type")}</th>
              <th scope="col" class="num">{t("hub.reports.col.items")}</th>
              <th scope="col" class="num">{t("hub.reports.col.amount")}</th>
            </tr>
          </thead>
          <tbody>
            {#each view.by_type as row (row.key)}
              <tr data-testid="type-{row.key}">
                <th scope="row">{t(TYPE[row.key] ?? "hub.reports.type.products")}</th>
                <td class="num tabular-nums">{row.count}</td>
                <td class="num tabular-nums">{formatMoney(row.amount_minor, locale)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </ScrollRegion>

      <h2 class="type-hub-body">{t("hub.reports.by_hour")}</h2>
      {#if hours.length === 0}
        <p class="type-hub-label note">{t("hub.reports.no_sales")}</p>
      {:else}
        <ScrollRegion label="hub.reports.by_hour">
          <table class="type-hub-body" data-testid="sales-by-hour">
            <caption class="sr-only">{t("hub.reports.by_hour")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("hub.reports.col.hour")}</th>
                <th scope="col" class="num">{t("hub.reports.col.bills")}</th>
                <th scope="col" class="num">{t("hub.reports.col.amount")}</th>
                <th scope="col"><span class="sr-only">{t("hub.reports.col.share")}</span></th>
              </tr>
            </thead>
            <tbody>
              {#each hours as h (h.hour)}
                <tr data-testid="hour-{h.hour}">
                  <th scope="row" class="tabular-nums">{hourText(h.hour)}</th>
                  <td class="num tabular-nums">{h.count}</td>
                  <td class="num tabular-nums">{formatMoney(h.amount_minor, locale)}</td>
                  <td class="bar-cell" aria-hidden="true">
                    <span class="bar" style="inline-size: {percentOf(h.amount_minor, peak)}%"
                    ></span>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </ScrollRegion>
      {/if}

      <h2 class="type-hub-body">{t("hub.reports.by_group")}</h2>
      {#if view.by_group.length === 0}
        <p class="type-hub-label note">{t("hub.reports.no_sales")}</p>
      {:else}
        <ScrollRegion label="hub.reports.by_group">
          <table class="type-hub-body" data-testid="sales-by-group">
            <caption class="sr-only">{t("hub.reports.by_group")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("hub.reports.col.group")}</th>
                <th scope="col" class="num">{t("hub.reports.col.bills")}</th>
                <th scope="col" class="num">{t("hub.reports.col.amount")}</th>
              </tr>
            </thead>
            <tbody>
              {#each view.by_group as g (g.id)}
                <tr data-testid="group-row">
                  <th scope="row">{groupName(g.name)}</th>
                  <td class="num tabular-nums">{g.count}</td>
                  <td class="num tabular-nums">{formatMoney(g.amount_minor, locale)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </ScrollRegion>
      {/if}

      <h2 class="type-hub-body">{t("hub.reports.by_category")}</h2>
      {#if view.by_category.length === 0}
        <p class="type-hub-label note">{t("hub.reports.no_products")}</p>
      {:else}
        <ScrollRegion label="hub.reports.by_category">
          <table class="type-hub-body" data-testid="sales-by-category">
            <caption class="sr-only">{t("hub.reports.by_category")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("hub.reports.col.category")}</th>
                <th scope="col" class="num">{t("hub.reports.col.items")}</th>
                <th scope="col" class="num">{t("hub.reports.col.amount")}</th>
              </tr>
            </thead>
            <tbody>
              {#each view.by_category as c (c.id)}
                <tr data-testid="category-row">
                  <th scope="row">{categoryName(c.name)}</th>
                  <td class="num tabular-nums">{c.count}</td>
                  <td class="num tabular-nums">{formatMoney(c.amount_minor, locale)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </ScrollRegion>
      {/if}

      <h2 class="type-hub-body">{t("hub.reports.received_title")}</h2>
      <p class="type-hub-label note">{t("hub.reports.received_note")}</p>
      <ScrollRegion label="hub.reports.received_title">
        <table class="type-hub-body" data-testid="sales-received">
          <caption class="sr-only">{t("hub.reports.received_title")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("hub.reports.col.type")}</th>
              <th scope="col" class="num">{t("hub.reports.col.payments")}</th>
              <th scope="col" class="num">{t("hub.reports.col.amount")}</th>
            </tr>
          </thead>
          <tbody>
            {#each view.received as row (row.key)}
              <tr data-testid="received-{row.key}">
                <th scope="row">{t(RECEIVED[row.key] ?? "hub.reports.received.topup")}</th>
                <td class="num tabular-nums">{row.count}</td>
                <td class="num tabular-nums">{formatMoney(row.amount_minor, locale)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </ScrollRegion>

      <ExportPanel report="daily-sales" selection={{ from: view.from, to: view.to }} />
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

  .totals {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-5);
  }

  .totals dt {
    color: var(--text-muted);
  }

  table {
    inline-size: 100%;
    max-inline-size: 48rem;
    border-collapse: collapse;
  }

  th,
  td {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border-strong);
    text-align: start;
    vertical-align: middle;
    font-weight: inherit;
  }

  .num {
    text-align: end;
  }

  .bar-cell {
    inline-size: 30%;
  }

  .bar {
    display: block;
    block-size: var(--space-2);
    border-radius: var(--radius-sm);
    background-color: var(--accent);
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
