<script lang="ts">
  import { Button, TabControl, TierLock, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, tick } from "svelte";
  import CashPanel from "./CashPanel.svelte";
  import ExportsPanel from "./ExportsPanel.svelte";
  import RevenuePanel from "./RevenuePanel.svelte";
  import SetupGate from "./SetupGate.svelte";
  import StockPanel from "./StockPanel.svelte";
  import {
    byCurrency,
    dateIn,
    downloadCsv,
    loadLevels,
    loadReport,
    rangeOf,
    rangeProblem,
    shiftsOf,
    stockViews,
    type BranchReport,
    type ExportKind,
    type LevelsWindow,
    type RangeKind,
  } from "./reports";
  import { age, isLive, liveBranches, refresh, shell } from "./shell.svelte";

  const RANGES = [
    ["today", "owner.reports.range.today"],
    ["week", "owner.reports.range.week"],
    ["month", "owner.reports.range.month"],
    ["custom", "owner.reports.range.custom"],
  ] as const satisfies readonly [RangeKind, MessageKey][];
  const TABS = [
    { id: "revenue", label: "owner.reports.tab.revenue" },
    { id: "cash", label: "owner.reports.tab.cash" },
    { id: "stock", label: "owner.reports.tab.stock" },
    { id: "exports", label: "owner.reports.tab.exports" },
  ] as const satisfies readonly { id: string; label: MessageKey }[];
  const ALL = "all";

  // Branch-scoped screens take their branches from the shared live list; Free ones are locked.
  const paid = $derived(liveBranches().filter((b) => b.tier !== "free"));
  const locked = $derived(liveBranches().filter((b) => b.tier === "free"));
  const pending = $derived((shell.branches ?? []).filter((b) => !isLive(b)));

  let tab = $state("revenue");
  let kind: RangeKind = $state("week");
  let branchId = $state(ALL);
  let custom = $state({ from: "", to: "" });
  let report: BranchReport[] = $state([]);
  let windows: LevelsWindow[] = $state([]);
  let windowsLoaded = $state(false);
  let loaded = $state(false);
  let failed = $state(false);
  let loading = $state(false);
  let exporting = $state(false);
  let printing = false;
  let exportFailed = $state(false);
  let now = $state(Date.now());
  let ticket = 0;
  let windowTicket = 0;

  const picked = $derived(
    branchId === ALL || paid.length < 2 ? paid : paid.filter((b) => b.id === branchId),
  );
  // "Today" is the date at the first picked branch (a custom range is typed by the owner).
  const today = $derived(dateIn(picked[0]?.timezone ?? "UTC", now));
  const range = $derived(rangeOf(kind, today, custom));
  const problem = $derived(rangeProblem(range.from, range.to));
  const views = $derived(byCurrency(report, range.from, range.to));
  const shifts = $derived(shiftsOf(report));
  const stock = $derived(stockViews(report, windows, today));
  const asOf = $derived(
    [...report.map((r) => r.asOf), ...(tab === "stock" ? windows.map((w) => w.asOf) : [])]
      .filter(Boolean)
      .sort()[0] ?? "",
  );
  const stale = $derived(
    report.length > 0 &&
      (report.some((r) => !r.fresh) || (tab === "stock" && windows.some((w) => !w.fresh))),
  );

  async function run() {
    const mine = ++ticket;
    if (!picked.length || problem || !range.from || !range.to) {
      loading = false;
      return;
    }
    now = Date.now();
    loading = true;
    failed = false;
    const res = await loadReport(picked, range.from, range.to);
    if (mine !== ticket) return;
    loading = false;
    if (res === "signed-out") {
      await refresh();
      return;
    }
    report = res;
    failed = res.every((r) => !r.range);
    loaded = true;
  }

  // The stock tab also reads the newest levels of the last week, once per branch pick.
  async function runLevels() {
    const mine = ++windowTicket;
    if (!picked.length) return;
    const res = await loadLevels(picked, today);
    if (mine !== windowTicket) return;
    if (res === "signed-out") {
      await refresh();
      return;
    }
    windows = res;
    windowsLoaded = true;
  }

  // Load again whenever the branches, the range or the pick change.
  $effect(() => {
    void [picked.map((b) => b.id).join(), range.from, range.to, problem];
    void run();
  });

  $effect(() => {
    void [picked.map((b) => b.id).join(), today];
    windowsLoaded = false;
    if (tab === "stock") void runLevels();
  });

  // Choosing Custom starts from this week, so both dates always hold something.
  $effect(() => {
    if (kind === "custom" && !custom.from) custom = rangeOf("week", today, custom);
  });

  onMount(() => {
    const id = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(id);
  });

  // Export → CSV: the cloud builds it from the same rollups; the browser saves the file.
  async function exportCsv(kind: ExportKind) {
    if (exporting || !picked.length) return;
    exporting = true;
    exportFailed = false;
    const res = await downloadCsv(kind, picked, range.from, range.to);
    exporting = false;
    if (res === "signed-out") await refresh();
    else exportFailed = res === "failed";
  }

  // Export → PDF: the report is shown with its print header, and the browser prints it to PDF.
  async function exportPdf(kind: ExportKind) {
    if (printing) return;
    printing = true;
    tab = kind;
    await tick();
    for (let i = 0; i < 100 && (loading || (kind === "stock" && !windowsLoaded)); i++) {
      await new Promise((r) => setTimeout(r, 100));
    }
    now = Date.now();
    await tick();
    // A report that failed to load is not worth printing under a valid-looking header.
    if (!failed) window.print();
    printing = false;
  }

  const generated = $derived(
    new Intl.DateTimeFormat("en-GB", {
      dateStyle: "medium",
      timeStyle: "short",
      timeZone: picked[0]?.timezone ?? "UTC",
    }).format(now),
  );

  const ago = (iso: string) => {
    const [key, n] = age(iso, now);
    return t(key, { n });
  };
</script>

{#if paid.length}
  <div class="controls">
    <div class="field">
      <label class="label" for="rep-range">{t("owner.reports.range")}</label>
      <select id="rep-range" class="ui-interactive select" bind:value={kind}>
        {#each RANGES as [id, label] (id)}<option value={id}>{t(label)}</option>{/each}
      </select>
    </div>
    <div class="field">
      <label class="label" for="rep-branch">{t("owner.reports.branch")}</label>
      <select id="rep-branch" class="ui-interactive select" bind:value={branchId}>
        {#if paid.length > 1}<option value={ALL}>{t("owner.reports.branch.all")}</option>{/if}
        {#each paid as b (b.id)}<option value={b.id}>{b.name}</option>{/each}
      </select>
    </div>
    {#if kind === "custom"}
      <div class="field">
        <label class="label" for="rep-from">{t("owner.reports.from")}</label>
        <input
          id="rep-from"
          class="ui-interactive select"
          type="date"
          max={today}
          bind:value={custom.from}
        />
      </div>
      <div class="field">
        <label class="label" for="rep-to">{t("owner.reports.to")}</label>
        <input
          id="rep-to"
          class="ui-interactive select"
          type="date"
          max={today}
          bind:value={custom.to}
        />
      </div>
    {/if}
  </div>
  {#if problem}<p role="alert">
      {t(problem === "order" ? "owner.reports.error.order" : "owner.reports.error.long")}
    </p>{/if}

  <div class="screen">
    <TabControl items={[...TABS]} label="owner.reports.tabs" bind:selected={tab} />
  </div>
  {#if tab !== "exports"}
    <div class="screen export" role="group" aria-label={t("owner.reports.export")}>
      <Button
        label="owner.reports.export.csv"
        variant="secondary"
        onclick={() => exportCsv(tab as ExportKind)}
        disabled={exporting}
        disabledReason="owner.reports.export.busy"
      />
      <Button
        label="owner.reports.export.pdf"
        variant="secondary"
        onclick={() => exportPdf(tab as ExportKind)}
      />
    </div>
  {/if}
  {#if exportFailed}<p role="alert" class="screen">{t("owner.reports.export.failed")}</p>{/if}
  <div class="print-header">
    <h2>{t(TABS.find((x) => x.id === tab)?.label ?? "owner.reports.tab.revenue")}</h2>
    <p>{t("owner.reports.print.branches", { names: picked.map((b) => b.name).join(", ") })}</p>
    <p>{t("owner.reports.print.range", { from: range.from, to: range.to })}</p>
    <p>{t("owner.reports.print.generated", { time: generated })}</p>
    {#if asOf}<p>{t("owner.reports.print.age", { age: ago(asOf) })}</p>{/if}
  </div>
  <div class="status" role="status">
    {#if loading}{t("owner.loading")}{:else if asOf && stale}{t("owner.updated", {
        age: ago(asOf),
      })}{/if}
  </div>

  <div
    role="tabpanel"
    aria-label={t(TABS.find((x) => x.id === tab)?.label ?? "owner.reports.tab.revenue")}
    class="panel"
  >
    {#if tab === "exports"}
      <ExportsPanel
        from={range.from}
        to={range.to}
        branches={picked.map((b) => b.name).join(", ")}
        busy={exporting}
        csv={exportCsv}
        pdf={exportPdf}
      />
    {:else if !loaded && !failed}
      <p>{t("owner.loading")}</p>
    {:else if failed}
      <p>{t("owner.unavailable")}</p>
      <Button label="common.retry" variant="secondary" onclick={run} />
    {:else if tab === "cash"}
      <CashPanel {shifts} many={picked.length > 1} />
    {:else if tab === "stock"}
      {#if windowsLoaded}
        <StockPanel views={stock} many={picked.length > 1} />
      {:else}
        <p>{t("owner.loading")}</p>
      {/if}
    {:else}
      <RevenuePanel {views} many={views.length > 1} {now} />
    {/if}
  </div>
{/if}

{#each locked as b (b.id)}
  <section aria-label={b.name}>
    <h2 class="type-hub-label">{b.name}</h2>
    <TierLock tier="Basic" />
  </section>
{/each}
{#each pending as b (b.id)}
  <section aria-label={b.name}>
    <h2 class="type-hub-label">{b.name}</h2>
    <SetupGate branch={b} />
  </section>
{/each}

<style>
  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    margin-bottom: var(--space-4);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .label {
    font-weight: var(--type-hub-title-weight);
  }
  .select {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--text-field-background);
    color: var(--text-field-foreground);
    font: inherit;
  }
  .status {
    min-height: 1.5em;
    margin: var(--space-2) 0;
    color: var(--text-muted);
  }
  .panel {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: var(--space-4);
  }
  h2 {
    margin: var(--space-4) 0 var(--space-2);
  }
  .export {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    margin-bottom: var(--space-2);
  }
  .print-header {
    display: none;
  }
  @media print {
    .controls,
    .screen,
    .status {
      display: none;
    }
    .print-header {
      display: block;
      margin-bottom: var(--space-4);
    }
    .print-header p {
      margin: 0;
    }
  }
</style>
