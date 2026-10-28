<script lang="ts">
  import { TabControl, formatBusinessDay, formatDateTime, formatMoney, t } from "@arena/ui";
  import type { ExpiryView, StockMoves, StockReport, WasteReport } from "@arena/ui/generated";
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
  import {
    MOVE_REASONS,
    businessDayNow,
    loadStockExpiring,
    loadStockLevels,
    loadStockMoves,
    loadStockWaste,
    type ReportOutcome,
  } from "../../../lib/reports";

  type Tab = "levels" | "moves" | "waste" | "expiring";

  const TABS: { id: Tab; label: MessageKey }[] = [
    { id: "levels", label: "hub.stock.tab.levels" },
    { id: "moves", label: "hub.stock.tab.moves" },
    { id: "waste", label: "hub.stock.tab.waste" },
    { id: "expiring", label: "hub.stock.tab.expiring" },
  ];

  const MOVE_LABELS: Record<(typeof MOVE_REASONS)[number], MessageKey> = {
    stock_in: "hub.stock.move.stock_in",
    stock_in_reverted: "hub.stock.move.stock_in_reverted",
    sale: "hub.stock.move.sale",
    sale_reverted: "hub.stock.move.sale_reverted",
    return: "hub.stock.move.return",
    waste: "hub.stock.move.waste",
    waste_reverted: "hub.stock.move.waste_reverted",
    expired: "hub.stock.move.expired",
    count_adjust: "hub.stock.move.count_adjust",
  };

  const WASTE_LABELS: Record<string, MessageKey> = {
    spoiled: "reason.spoiled",
    broken: "reason.broken",
    free: "reason.free",
    cancelled_order: "reason.cancelled_order",
    expired: "hub.stock.waste.expired",
  };

  const locale = $derived(branchLocale.current);
  const allowed = $derived(hasPermission(sessionStaff(page.data), "report.view"));

  let tab = $state<Tab>("levels");
  let from = $state("");
  let to = $state("");
  let product = $state("");
  let reason = $state("");
  let levels = $state<StockReport | null>(null);
  let moves = $state<StockMoves | null>(null);
  let waste = $state<WasteReport | null>(null);
  let expiry = $state<ExpiryView | null>(null);
  let problem: MessageKey | null = $state(null);
  let loading = $state(false);
  let turn = 0;

  /** Shows the answer of the load started as `mine`, unless a newer load has started since. */
  async function settle<T>(mine: number, outcome: ReportOutcome<T>, apply: (view: T) => void) {
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
    if (outcome.kind === "ok") apply(outcome.view);
  }

  /** Loads the view of the open tab. The product list for the filter comes from On hand. */
  async function load() {
    const mine = ++turn;
    loading = true;
    if (tab === "levels") {
      await settle(mine, await loadStockLevels(), (v) => (levels = v));
    } else if (tab === "moves") {
      const [list, found] = await Promise.all([
        levels ? null : loadStockLevels(),
        loadStockMoves(from, to, product, reason),
      ]);
      if (list?.kind === "ok") levels = list.view;
      await settle(mine, found, (v) => (moves = v));
    } else if (tab === "waste") {
      await settle(mine, await loadStockWaste(from, to), (v) => (waste = v));
    } else {
      await settle(mine, await loadStockExpiring(), (v) => (expiry = v));
    }
  }

  // Opens on today once the branch locale (its time zone and day start) and the session are in.
  $effect(() => {
    if (locale && allowed && !from) {
      from = to = businessDayNow(locale, Date.now());
      untrack(() => void load());
    }
  });

  const moveLabel = (key: string) =>
    key in MOVE_LABELS ? t(MOVE_LABELS[key as keyof typeof MOVE_LABELS]) : key;

  const wasteLabel = (key: string) => (key in WASTE_LABELS ? t(WASTE_LABELS[key]) : key);

  /** A signed quantity or amount: stock in is positive, stock out negative. */
  const signed = (n: number, text: string) => (n > 0 ? `+${text}` : text); // i18n-ignore

  /** The two Expiring lists: lots inside the warning window, and expired lots still on the shelf. */
  const expiryLists = $derived(
    expiry
      ? [
          {
            id: "expiring",
            title: "hub.stock.expiring_title" as MessageKey,
            none: "hub.stock.expiring_none" as MessageKey,
            lots: expiry.expiring,
          },
          {
            id: "expired",
            title: "hub.stock.expired_title" as MessageKey,
            none: "hub.stock.expired_none" as MessageKey,
            lots: expiry.expired,
          },
        ]
      : [],
  );
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.reports.stock") })}</title>
</svelte:head>

<section class="report" data-testid="stock-report-screen">
  <h1 class="type-hub-numeric">{t("hub.reports.stock")}</h1>
  <p class="type-hub-label"><a href="/reports">{t("hub.reports.back")}</a></p>

  {#if !allowed}
    <p class="type-hub-body note" role="status" data-testid="reports-denied">
      {t("hub.reports.no_access")}
    </p>
  {:else}
    <TabControl
      items={TABS}
      label="hub.stock.views"
      selected={tab}
      onchange={(id) => {
        tab = id as Tab;
        void load();
      }}
    >
      {#snippet panel(current)}
        {#if current === "moves" || current === "waste"}
          <ReportRange bind:from bind:to onchange={() => void load()} />
        {/if}

        {#if current === "moves"}
          <div class="filters">
            <label class="field type-hub-label">
              <span>{t("hub.stock.filter_product")}</span>
              <select
                class="ui-interactive"
                value={product}
                onchange={(e) => {
                  product = e.currentTarget.value;
                  void load();
                }}
                data-testid="stock-filter-product"
              >
                <option value="">{t("hub.stock.all_products")}</option>
                {#each levels?.items ?? [] as item (item.product_id)}
                  <option value={item.product_id}>{item.name}</option>
                {/each}
              </select>
            </label>
            <label class="field type-hub-label">
              <span>{t("hub.stock.filter_reason")}</span>
              <select
                class="ui-interactive"
                value={reason}
                onchange={(e) => {
                  reason = e.currentTarget.value;
                  void load();
                }}
                data-testid="stock-filter-reason"
              >
                <option value="">{t("hub.stock.all_reasons")}</option>
                {#each MOVE_REASONS as key (key)}
                  <option value={key}>{moveLabel(key)}</option>
                {/each}
              </select>
            </label>
          </div>
        {/if}

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

        {#if locale}
          {#if current === "levels" && levels}
            {#if levels.items.length === 0}
              <p class="type-hub-body note" data-testid="stock-none">{t("hub.stock.none")}</p>
            {:else}
              <ScrollRegion label="hub.stock.tab.levels">
                <table class="type-hub-body" data-testid="stock-table">
                  <caption class="sr-only">{t("hub.stock.tab.levels")}</caption>
                  <thead>
                    <tr>
                      <th scope="col">{t("hub.stock.col.product")}</th>
                      <th scope="col" class="num">{t("hub.stock.col.qty")}</th>
                      <th scope="col" class="num">{t("hub.stock.col.avg_cost")}</th>
                      <th scope="col" class="num">{t("hub.stock.col.value")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each levels.items as item (item.product_id)}
                      <tr data-testid="stock-row-{item.product_id}">
                        <th scope="row">{item.name}</th>
                        <td class="num tabular-nums">{item.qty}</td>
                        <td class="num tabular-nums">{formatMoney(item.avg_cost_minor, locale)}</td>
                        <td class="num tabular-nums" data-testid="stock-value">
                          {formatMoney(item.value_minor, locale)}
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                  <tfoot>
                    <tr>
                      <th scope="row" colspan="3">{t("hub.stock.total")}</th>
                      <td class="num tabular-nums" data-testid="stock-total">
                        {formatMoney(levels.total_value_minor, locale)}
                      </td>
                    </tr>
                  </tfoot>
                </table>
              </ScrollRegion>
            {/if}
          {:else if current === "moves" && moves}
            {#if moves.moves.length === 0}
              <p class="type-hub-body note" data-testid="moves-none">{t("hub.stock.moves_none")}</p>
            {:else}
              {#if moves.truncated}
                <p class="type-hub-label note" data-testid="moves-cut">
                  {t("hub.stock.moves_cut", { shown: moves.moves.length, total: moves.total })}
                </p>
              {/if}
              <ScrollRegion label="hub.stock.tab.moves">
                <table class="type-hub-body" data-testid="moves-table">
                  <caption class="sr-only">{t("hub.stock.tab.moves")}</caption>
                  <thead>
                    <tr>
                      <th scope="col">{t("hub.stock.col.time")}</th>
                      <th scope="col">{t("hub.stock.col.product")}</th>
                      <th scope="col">{t("hub.stock.col.reason")}</th>
                      <th scope="col" class="num">{t("hub.stock.col.moved")}</th>
                      <th scope="col" class="num">{t("hub.stock.col.value")}</th>
                      <th scope="col">{t("hub.stock.col.staff")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each moves.moves as m (m.move_id)}
                      <tr data-testid="move-row-{m.move_id}">
                        <td class="tabular-nums">{formatDateTime(m.at, locale.tz)}</td>
                        <th scope="row">{m.product}</th>
                        <td>
                          {moveLabel(m.reason)}
                          {#if m.waste_reason}
                            <span class="note">· {wasteLabel(m.waste_reason)}</span>
                          {/if}
                        </td>
                        <td class="num tabular-nums" data-testid="move-qty">
                          {signed(m.qty, String(m.qty))}
                        </td>
                        <td class="num tabular-nums" data-testid="move-value">
                          {signed(m.value_minor, formatMoney(m.value_minor, locale))}
                        </td>
                        <td class:muted={!m.staff}>{m.staff || t("hub.reports.former_staff")}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </ScrollRegion>
            {/if}
          {:else if current === "waste" && waste}
            <p class="type-hub-label note">{t("hub.stock.waste_note")}</p>
            <h2 class="type-hub-body">{t("hub.stock.waste_by_reason")}</h2>
            <ScrollRegion label="hub.stock.waste_by_reason">
              <table class="type-hub-body" data-testid="waste-reasons">
                <caption class="sr-only">{t("hub.stock.waste_by_reason")}</caption>
                <thead>
                  <tr>
                    <th scope="col">{t("hub.stock.col.reason")}</th>
                    <th scope="col" class="num">{t("hub.stock.col.moved")}</th>
                    <th scope="col" class="num">{t("hub.stock.col.value")}</th>
                  </tr>
                </thead>
                <tbody>
                  {#each waste.by_reason as r (r.reason)}
                    <tr data-testid="waste-reason-{r.reason}">
                      <th scope="row">{wasteLabel(r.reason)}</th>
                      <td class="num tabular-nums" data-testid="waste-qty">{r.qty}</td>
                      <td class="num tabular-nums" data-testid="waste-value">
                        {formatMoney(r.value_minor, locale)}
                      </td>
                    </tr>
                  {/each}
                </tbody>
                <tfoot>
                  <tr>
                    <th scope="row">{t("hub.stock.waste_total")}</th>
                    <td class="num tabular-nums" data-testid="waste-total-qty">{waste.total_qty}</td
                    >
                    <td class="num tabular-nums" data-testid="waste-total-value">
                      {formatMoney(waste.total_value_minor, locale)}
                    </td>
                  </tr>
                </tfoot>
              </table>
            </ScrollRegion>

            <h2 class="type-hub-body">{t("hub.stock.waste_by_product")}</h2>
            {#if waste.by_product.length === 0}
              <p class="type-hub-label note" data-testid="waste-none">
                {t("hub.stock.waste_none")}
              </p>
            {:else}
              <ScrollRegion label="hub.stock.waste_by_product">
                <table class="type-hub-body" data-testid="waste-products">
                  <caption class="sr-only">{t("hub.stock.waste_by_product")}</caption>
                  <thead>
                    <tr>
                      <th scope="col">{t("hub.stock.col.product")}</th>
                      <th scope="col">{t("hub.stock.col.reason")}</th>
                      <th scope="col" class="num">{t("hub.stock.col.moved")}</th>
                      <th scope="col" class="num">{t("hub.stock.col.value")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each waste.by_product as p (p.product_id)}
                      <tr data-testid="waste-product-{p.product_id}">
                        <th scope="row">{p.product}</th>
                        <td>
                          <ul class="split">
                            {#each p.reasons as r (r.reason)}
                              <li>
                                {t("hub.stock.waste_split", {
                                  reason: wasteLabel(r.reason),
                                  qty: r.qty,
                                })}
                              </li>
                            {/each}
                          </ul>
                        </td>
                        <td class="num tabular-nums">{p.qty}</td>
                        <td class="num tabular-nums">{formatMoney(p.value_minor, locale)}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </ScrollRegion>
            {/if}
          {:else if current === "expiring" && expiry}
            <p class="type-hub-label note">
              {t("hub.stock.expiring_note", {
                days: expiry.warn_days,
                today: formatBusinessDay(expiry.today),
              })}
            </p>
            {#each expiryLists as list (list.id)}
              <h2 class="type-hub-body">{t(list.title)}</h2>
              {#if list.lots.length === 0}
                <p class="type-hub-label note" data-testid="{list.id}-none">{t(list.none)}</p>
              {:else}
                <ScrollRegion label={list.title}>
                  <table class="type-hub-body" data-testid="{list.id}-table">
                    <caption class="sr-only">{t(list.title)}</caption>
                    <thead>
                      <tr>
                        <th scope="col">{t("hub.stock.col.product")}</th>
                        <th scope="col">{t("hub.stock.col.expires")}</th>
                        <th scope="col" class="num">{t("hub.stock.col.moved")}</th>
                        <th scope="col" class="num">{t("hub.stock.col.cost")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each list.lots as lot (lot.lot_id)}
                        <tr data-testid="{list.id}-row-{lot.lot_id}">
                          <th scope="row">{lot.product}</th>
                          <td class="tabular-nums">{formatBusinessDay(lot.expiry)}</td>
                          <td class="num tabular-nums">{lot.qty}</td>
                          <td class="num tabular-nums">{formatMoney(lot.value_minor, locale)}</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </ScrollRegion>
              {/if}
            {/each}
          {/if}
        {/if}

        {#if current === "levels" && levels}
          <ExportPanel report="stock-on-hand" />
        {:else if current === "moves" && moves}
          <ExportPanel
            report="stock-moves"
            selection={{ from: moves.from, to: moves.to, product, reason }}
          />
        {:else if current === "waste" && waste}
          <ExportPanel report="stock-waste" selection={{ from: waste.from, to: waste.to }} />
        {:else if current === "expiring" && expiry}
          <ExportPanel report="stock-expiring" />
        {/if}
      {/snippet}
    </TabControl>
  {/if}
</section>

<style>
  .report {
    display: grid;
    gap: var(--space-3);
  }

  h1,
  h2,
  p {
    margin: 0;
  }

  .note,
  .muted {
    color: var(--text-muted);
  }

  .warn {
    color: var(--text-field-error-text);
  }

  .split {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  select {
    --ui-bg: var(--text-field-background);
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
    font-size: var(--type-web-body-size);
  }

  table {
    inline-size: 100%;
    max-inline-size: 60rem;
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

  tfoot th,
  tfoot td {
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
