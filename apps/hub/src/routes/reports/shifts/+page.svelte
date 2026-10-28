<script lang="ts">
  import { formatDateTime, formatMoney, t } from "@arena/ui";
  import type { ShiftListRow } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { SESSION_DEPENDENCY, hasPermission } from "../../../lib/auth";
  import { branchLocale } from "../../../lib/locale.svelte";
  import { sessionStaff } from "../../../lib/operating";
  import { loadShiftList } from "../../../lib/reports";
  import ScrollRegion from "../../../lib/ScrollRegion.svelte";

  const locale = $derived(branchLocale.current);
  const allowed = $derived(hasPermission(sessionStaff(page.data), "report.view"));

  let shifts = $state<ShiftListRow[] | null>(null);
  let problem: MessageKey | null = $state(null);
  let loading = $state(false);

  async function load() {
    loading = true;
    const outcome = await loadShiftList();
    loading = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    problem =
      outcome.kind === "ok"
        ? null
        : outcome.kind === "denied"
          ? "hub.reports.no_access"
          : "hub.reports.failed";
    if (outcome.kind === "ok") shifts = outcome.view.shifts;
  }

  onMount(() => {
    if (allowed) void load();
  });
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.reports.shifts") })}</title>
</svelte:head>

<section class="report" data-testid="shifts-report-screen">
  <h1 class="type-hub-numeric">{t("hub.reports.shifts")}</h1>
  <p class="type-hub-label"><a href="/reports">{t("hub.reports.back")}</a></p>

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

    {#if shifts && locale}
      {#if shifts.length === 0}
        <p class="type-hub-body note" data-testid="shifts-none">{t("hub.reports.no_shifts")}</p>
      {:else}
        <ScrollRegion label="hub.reports.shifts">
          <table class="type-hub-body" data-testid="shifts-table">
            <caption class="sr-only">{t("hub.reports.shifts")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("hub.reports.col.opened")}</th>
                <th scope="col">{t("hub.reports.col.closed")}</th>
                <th scope="col">{t("hub.reports.col.opened_by")}</th>
                <th scope="col">{t("hub.reports.col.closed_by")}</th>
                <th scope="col" class="num">{t("hub.reports.col.counted")}</th>
                <th scope="col" class="num">{t("hub.reports.col.variance")}</th>
                <th scope="col"><span class="sr-only">{t("hub.reports.col.open")}</span></th>
              </tr>
            </thead>
            <tbody>
              {#each shifts as s (s.shift_id)}
                <tr data-testid="shift-row">
                  <th scope="row" class="tabular-nums">{formatDateTime(s.opened_at, locale.tz)}</th>
                  <td class="tabular-nums">{formatDateTime(s.closed_at, locale.tz)}</td>
                  <td>{s.opened_by}</td>
                  <td>{s.closed_by}</td>
                  <td class="num tabular-nums">{formatMoney(s.counted_minor, locale)}</td>
                  <td class="num tabular-nums">{formatMoney(s.variance_minor, locale)}</td>
                  <td>
                    <a
                      class="ui-interactive open"
                      href="/reports/shifts/{s.shift_id}"
                      data-testid="shift-open-{s.shift_id}"
                    >
                      {t("hub.reports.open_z")}
                    </a>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </ScrollRegion>
      {/if}
    {/if}
  {/if}
</section>

<style>
  .report {
    display: grid;
    gap: var(--space-3);
  }

  h1,
  p {
    margin: 0;
  }

  .note {
    color: var(--text-muted);
  }

  .warn {
    color: var(--text-field-error-text);
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

  .num {
    text-align: end;
  }

  .open {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    color: var(--text);
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
