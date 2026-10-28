<script lang="ts">
  import { formatMoney, t } from "@arena/ui";
  import type { SessionsReport } from "@arena/ui/generated";
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
    bpText,
    businessDayNow,
    durationText,
    hourText,
    loadSessionsReport,
  } from "../../../lib/reports";

  const locale = $derived(branchLocale.current);
  const allowed = $derived(hasPermission(sessionStaff(page.data), "report.view"));

  let from = $state("");
  let to = $state("");
  let view = $state<SessionsReport | null>(null);
  let problem: MessageKey | null = $state(null);
  let loading = $state(false);
  let turn = 0;

  async function load() {
    const mine = ++turn;
    loading = true;
    const outcome = await loadSessionsReport(from, to);
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

  /** The hours any group was in use, so the utilisation grid skips the empty small hours. */
  const hours = $derived(
    Array.from({ length: 24 }, (_, hour) => hour).filter((hour) =>
      (view?.by_group ?? []).some((g) => (g.utilisation_bp[hour] ?? 0) > 0),
    ),
  );
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.reports.sessions") })}</title>
</svelte:head>

<section class="report" data-testid="sessions-report-screen">
  <h1 class="type-hub-numeric">{t("hub.reports.sessions")}</h1>
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
      <dl class="totals" data-testid="sessions-totals">
        <div>
          <dt class="type-hub-label">{t("hub.reports.col.sessions")}</dt>
          <dd class="type-hub-numeric" data-testid="sessions-count">{view.sessions}</dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.reports.col.billed")}</dt>
          <dd class="type-hub-numeric" data-testid="sessions-minutes">
            {durationText(view.minutes)}
          </dd>
        </div>
        <div>
          <dt class="type-hub-label">{t("hub.reports.col.revenue")}</dt>
          <dd class="type-hub-numeric" data-testid="sessions-revenue">
            {formatMoney(view.revenue_minor, locale)}
          </dd>
        </div>
      </dl>

      {#if view.sessions === 0}
        <p class="type-hub-body note" data-testid="sessions-none">
          {t("hub.reports.no_sessions")}
        </p>
      {/if}

      <h2 class="type-hub-body">{t("hub.reports.by_group")}</h2>
      <ScrollRegion label="hub.reports.by_group">
        <table class="type-hub-body" data-testid="sessions-by-group">
          <caption class="sr-only">{t("hub.reports.by_group")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("hub.reports.col.group")}</th>
              <th scope="col" class="num">{t("hub.reports.col.devices")}</th>
              <th scope="col" class="num">{t("hub.reports.col.sessions")}</th>
              <th scope="col" class="num">{t("hub.reports.col.billed")}</th>
              <th scope="col" class="num">{t("hub.reports.col.revenue")}</th>
            </tr>
          </thead>
          <tbody>
            {#each view.by_group as g (g.group_id)}
              <tr data-testid="group-row">
                <th scope="row">{g.name}</th>
                <td class="num tabular-nums">{g.devices}</td>
                <td class="num tabular-nums">{g.sessions}</td>
                <td class="num tabular-nums">{durationText(g.minutes)}</td>
                <td class="num tabular-nums">{formatMoney(g.revenue_minor, locale)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </ScrollRegion>

      <h2 class="type-hub-body">{t("hub.reports.utilisation")}</h2>
      <p class="type-hub-label note">{t("hub.reports.utilisation_note")}</p>
      {#if hours.length === 0}
        <p class="type-hub-label note" data-testid="utilisation-none">
          {t("hub.reports.no_sessions")}
        </p>
      {:else}
        <ScrollRegion label="hub.reports.utilisation">
          <table class="type-hub-body" data-testid="sessions-utilisation">
            <caption class="sr-only">{t("hub.reports.utilisation")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("hub.reports.col.hour")}</th>
                {#each view.by_group as g (g.group_id)}
                  <th scope="col" class="num">{g.name}</th>
                {/each}
              </tr>
            </thead>
            <tbody>
              {#each hours as hour (hour)}
                <tr data-testid="utilisation-{hour}">
                  <th scope="row" class="tabular-nums">{hourText(hour)}</th>
                  {#each view.by_group as g (g.group_id)}
                    <td class="num tabular-nums">{bpText(g.utilisation_bp[hour] ?? 0)}</td>
                  {/each}
                </tr>
              {/each}
            </tbody>
          </table>
        </ScrollRegion>
      {/if}

      <h2 class="type-hub-body">{t("hub.reports.by_device")}</h2>
      <ScrollRegion label="hub.reports.by_device">
        <table class="type-hub-body" data-testid="sessions-by-device">
          <caption class="sr-only">{t("hub.reports.by_device")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("hub.reports.col.device")}</th>
              <th scope="col" class="num">{t("hub.reports.col.sessions")}</th>
              <th scope="col" class="num">{t("hub.reports.col.billed")}</th>
              <th scope="col" class="num">{t("hub.reports.col.revenue")}</th>
            </tr>
          </thead>
          <tbody>
            {#each view.by_device as d (d.device_id)}
              <tr data-testid="device-row">
                <th scope="row">{d.label}</th>
                <td class="num tabular-nums">{d.sessions}</td>
                <td class="num tabular-nums">{durationText(d.minutes)}</td>
                <td class="num tabular-nums">{formatMoney(d.revenue_minor, locale)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </ScrollRegion>

      <ExportPanel report="sessions" selection={{ from: view.from, to: view.to }} />
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

  .sr-only {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
