<script lang="ts">
  import { formatDateTime, formatMoney, t } from "@arena/ui";
  import type { StaffReport } from "@arena/ui/generated";
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
  import { businessDayNow, loadStaffReport } from "../../../lib/reports";

  const locale = $derived(branchLocale.current);
  const allowed = $derived(hasPermission(sessionStaff(page.data), "report.view"));

  let from = $state("");
  let to = $state("");
  let view = $state<StaffReport | null>(null);
  let problem: MessageKey | null = $state(null);
  let loading = $state(false);
  let turn = 0;

  async function load() {
    const mine = ++turn;
    loading = true;
    const outcome = await loadStaffReport(from, to);
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

  /** A member no longer in the staff table comes with an empty name. */
  const who = (name: string) => name || t("hub.reports.former_staff");

  /** A count and the value it came to, such as `2 · ₹150.00`. */
  const countValue = (count: number, minor: number) =>
    t("hub.reports.count_value", { count, value: formatMoney(minor, locale!) });

  /** A signed variance: short is negative, over is positive. */
  const signed = (minor: number) =>
    minor > 0 ? `+${formatMoney(minor, locale!)}` : formatMoney(minor, locale!); // i18n-ignore

  const shifts = $derived(
    (view?.staff ?? []).flatMap((s) =>
      s.shifts.map((v) => ({ ...v, staff_id: s.staff_id, name: s.name })),
    ),
  );
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.reports.staff") })}</title>
</svelte:head>

<section class="report" data-testid="staff-report-screen">
  <h1 class="type-hub-numeric">{t("hub.reports.staff")}</h1>
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
      {#if view.staff.length === 0}
        <p class="type-hub-body note" data-testid="staff-none">{t("hub.reports.no_staff")}</p>
      {:else}
        <ScrollRegion label="hub.reports.staff">
          <table class="type-hub-body" data-testid="staff-table">
            <caption class="sr-only">{t("hub.reports.staff")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("hub.reports.col.staff")}</th>
                <th scope="col" class="num">{t("hub.reports.col.sales")}</th>
                <th scope="col" class="num">{t("hub.reports.col.started")}</th>
                <th scope="col" class="num">{t("hub.reports.col.discounts")}</th>
                <th scope="col" class="num">{t("hub.reports.col.voids")}</th>
                <th scope="col" class="num">{t("hub.reports.col.refunds_approved")}</th>
                <th scope="col" class="num">{t("hub.reports.col.variance")}</th>
              </tr>
            </thead>
            <tbody>
              {#each view.staff as s (s.staff_id)}
                <tr data-testid="staff-row-{s.staff_id}">
                  <th scope="row" class:muted={!s.name}>{who(s.name)}</th>
                  <td class="num tabular-nums" data-testid="staff-sales">
                    {countValue(s.sales_count, s.sales_minor)}
                  </td>
                  <td class="num tabular-nums" data-testid="staff-sessions">
                    {s.sessions_started}
                  </td>
                  <td class="num tabular-nums" data-testid="staff-discounts">
                    {countValue(s.discounts_count, s.discounts_minor)}
                  </td>
                  <td class="num tabular-nums" data-testid="staff-voids">
                    {countValue(s.voids_count, s.voids_minor)}
                  </td>
                  <td class="num tabular-nums" data-testid="staff-refunds">
                    {countValue(s.refunds_count, s.refunds_minor)}
                  </td>
                  <td class="num tabular-nums" data-testid="staff-variance">
                    {signed(s.variance_minor)}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </ScrollRegion>

        <h2 class="type-hub-body">{t("hub.reports.shift_variance")}</h2>
        {#if shifts.length === 0}
          <p class="type-hub-label note" data-testid="shifts-none">
            {t("hub.reports.no_shift_variance")}
          </p>
        {:else}
          <ScrollRegion label="hub.reports.shift_variance">
            <table class="type-hub-body" data-testid="staff-shifts">
              <caption class="sr-only">{t("hub.reports.shift_variance")}</caption>
              <thead>
                <tr>
                  <th scope="col">{t("hub.reports.col.staff")}</th>
                  <th scope="col">{t("hub.reports.col.shift_closed")}</th>
                  <th scope="col" class="num">{t("hub.reports.col.variance")}</th>
                  <th scope="col"><span class="sr-only">{t("hub.reports.col.open")}</span></th>
                </tr>
              </thead>
              <tbody>
                {#each shifts as v (v.shift_id)}
                  <tr data-testid="shift-variance-row">
                    <th scope="row" class:muted={!v.name}>{who(v.name)}</th>
                    <td class="tabular-nums">{formatDateTime(v.closed_at, locale.tz)}</td>
                    <td class="num tabular-nums" data-testid="shift-variance">
                      {signed(v.variance_minor)}
                    </td>
                    <td>
                      <a
                        class="ui-interactive open"
                        href="/reports/shifts/{v.shift_id}"
                        data-testid="shift-open-{v.shift_id}"
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

      <ExportPanel report="staff" selection={{ from: view.from, to: view.to }} />
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
