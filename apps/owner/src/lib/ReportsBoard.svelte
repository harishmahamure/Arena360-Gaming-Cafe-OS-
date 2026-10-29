<script lang="ts">
  import { Button, TabControl, TierLock, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import CashPanel from "./CashPanel.svelte";
  import RevenuePanel from "./RevenuePanel.svelte";
  import SetupGate from "./SetupGate.svelte";
  import {
    byCurrency,
    dateIn,
    loadReport,
    rangeOf,
    rangeProblem,
    shiftsOf,
    type BranchReport,
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
  let loaded = $state(false);
  let failed = $state(false);
  let loading = $state(false);
  let now = $state(Date.now());
  let ticket = 0;

  const picked = $derived(
    branchId === ALL || paid.length < 2 ? paid : paid.filter((b) => b.id === branchId),
  );
  // "Today" is the date at the first picked branch (a custom range is typed by the owner).
  const today = $derived(dateIn(picked[0]?.timezone ?? "UTC", now));
  const range = $derived(rangeOf(kind, today, custom));
  const problem = $derived(rangeProblem(range.from, range.to));
  const views = $derived(byCurrency(report, range.from, range.to));
  const shifts = $derived(shiftsOf(report));
  const asOf = $derived(
    report
      .map((r) => r.asOf)
      .filter(Boolean)
      .sort()[0] ?? "",
  );
  const stale = $derived(report.length > 0 && report.some((r) => !r.fresh));

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

  // Load again whenever the branches, the range or the pick change.
  $effect(() => {
    void [picked.map((b) => b.id).join(), range.from, range.to, problem];
    void run();
  });

  // Choosing Custom starts from this week, so both dates always hold something.
  $effect(() => {
    if (kind === "custom" && !custom.from) custom = rangeOf("week", today, custom);
  });

  onMount(() => {
    const id = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(id);
  });

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

  <TabControl items={[...TABS]} label="owner.reports.tabs" bind:selected={tab} />
  <div class="status" role="status">
    {#if loading}{t("owner.loading")}{:else if asOf && stale}{t("owner.updated", {
        age: ago(asOf),
      })}{/if}
  </div>

  <div
    role="tabpanel"
    aria-label={t(tab === "cash" ? "owner.reports.tab.cash" : "owner.reports.tab.revenue")}
    class="panel"
  >
    {#if !loaded && !failed}
      <p>{t("owner.loading")}</p>
    {:else if failed}
      <p>{t("owner.unavailable")}</p>
      <Button label="common.retry" variant="secondary" onclick={run} />
    {:else if tab === "cash"}
      <CashPanel {shifts} many={picked.length > 1} />
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
    gap: var(--space-4);
  }
  h2 {
    margin: var(--space-4) 0 var(--space-2);
  }
</style>
