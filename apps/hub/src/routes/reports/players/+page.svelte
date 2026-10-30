<script lang="ts">
  import { formatMoney, t } from "@arena/ui";
  import type { PlayersReport } from "@arena/ui/generated";
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
  import { businessDayNow, loadPlayersReport } from "../../../lib/reports";

  const locale = $derived(branchLocale.current);
  const allowed = $derived(hasPermission(sessionStaff(page.data), "report.view"));

  let from = $state("");
  let to = $state("");
  let view = $state<PlayersReport | null>(null);
  let problem: MessageKey | null = $state(null);
  let loading = $state(false);
  let turn = 0;

  async function load() {
    const mine = ++turn;
    loading = true;
    const outcome = await loadPlayersReport(from, to);
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

  const BUCKET_LABELS: Record<string, MessageKey> = {
    "0_7": "hub.reports.credit_0_7",
    "8_14": "hub.reports.credit_8_14",
    "15_plus": "hub.reports.credit_15_plus",
  };

  const bucketLabel = (key: string): MessageKey => BUCKET_LABELS[key] ?? "hub.reports.col.age";

  /** The name to show: an erased player has none, and nothing else about them is sent. */
  const who = (p: { name: string; erased: boolean }) =>
    p.erased ? t("hub.reports.erased_player") : p.name;
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.reports.players") })}</title>
</svelte:head>

<section class="report" data-testid="players-report-screen">
  <h1 class="type-hub-numeric">{t("hub.reports.players")}</h1>
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
      <h2 class="type-hub-body">{t("hub.reports.top_spenders")}</h2>
      {#if view.top.length === 0}
        <p class="type-hub-body note" data-testid="players-none">{t("hub.reports.no_players")}</p>
      {:else}
        <ScrollRegion label="hub.reports.top_spenders">
          <table class="type-hub-body" data-testid="players-top">
            <caption class="sr-only">{t("hub.reports.top_spenders")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("hub.reports.col.player")}</th>
                <th scope="col" class="num">{t("hub.reports.col.visits")}</th>
                <th scope="col" class="num">{t("hub.reports.col.spend")}</th>
              </tr>
            </thead>
            <tbody>
              {#each view.top as p (p.player_id)}
                <tr data-testid="top-row" data-erased={p.erased}>
                  <th scope="row" class:muted={p.erased}
                    >{who(p)}
                    {#if p.erased}<span class="muted type-hub-label"
                        >{t("hub.players.erased.id", { id: p.player_id })}</span
                      >{/if}</th
                  >
                  <td class="num tabular-nums">{p.visits}</td>
                  <td class="num tabular-nums">{formatMoney(p.spend_minor, locale)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </ScrollRegion>
      {/if}

      <h2 class="type-hub-body">{t("hub.reports.new_returning")}</h2>
      <ScrollRegion label="hub.reports.new_returning">
        <table class="type-hub-body" data-testid="players-split">
          <caption class="sr-only">{t("hub.reports.new_returning")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("hub.reports.col.type")}</th>
              <th scope="col" class="num">{t("hub.reports.col.players")}</th>
              <th scope="col" class="num">{t("hub.reports.col.spend")}</th>
            </tr>
          </thead>
          <tbody>
            <tr data-testid="new-row">
              <th scope="row">{t("hub.reports.new_players")}</th>
              <td class="num tabular-nums" data-testid="new-count">{view.new_players.players}</td>
              <td class="num tabular-nums" data-testid="new-spend">
                {formatMoney(view.new_players.spend_minor, locale)}
              </td>
            </tr>
            <tr data-testid="returning-row">
              <th scope="row">{t("hub.reports.returning_players")}</th>
              <td class="num tabular-nums" data-testid="returning-count">
                {view.returning_players.players}
              </td>
              <td class="num tabular-nums" data-testid="returning-spend">
                {formatMoney(view.returning_players.spend_minor, locale)}
              </td>
            </tr>
            <tr data-testid="guest-row">
              <th scope="row">{t("hub.reports.guest")}</th>
              <td class="num tabular-nums" data-testid="guest-bills">{view.guest.bills}</td>
              <td class="num tabular-nums" data-testid="guest-spend">
                {formatMoney(view.guest.spend_minor, locale)}
              </td>
            </tr>
          </tbody>
        </table>
      </ScrollRegion>
      <p class="type-hub-label note">{t("hub.reports.guest_note")}</p>

      <h2 class="type-hub-body">{t("hub.reports.credit_title")}</h2>
      <p class="type-hub-label note">{t("hub.reports.credit_note", { day: view.credit_as_of })}</p>
      <dl class="totals" data-testid="credit-buckets">
        <div>
          <dt class="type-hub-label">{t("hub.reports.credit_total")}</dt>
          <dd class="type-hub-numeric" data-testid="credit-total">
            {formatMoney(view.credit_outstanding_minor, locale)}
          </dd>
        </div>
        {#each view.credit_buckets as b (b.key)}
          <div>
            <dt class="type-hub-label">{t(bucketLabel(b.key))}</dt>
            <dd class="type-hub-numeric" data-testid="credit-bucket-{b.key}">
              {formatMoney(b.amount_minor, locale)}
            </dd>
          </div>
        {/each}
      </dl>

      <h2 class="type-hub-body">{t("hub.reports.debtors")}</h2>
      {#if view.debtors.length === 0}
        <p class="type-hub-body note" data-testid="debtors-none">{t("hub.reports.no_debtors")}</p>
      {:else}
        <ScrollRegion label="hub.reports.debtors">
          <table class="type-hub-body" data-testid="players-debtors">
            <caption class="sr-only">{t("hub.reports.debtors")}</caption>
            <thead>
              <tr>
                <th scope="col">{t("hub.reports.col.player")}</th>
                <th scope="col" class="num">{t("hub.reports.col.owed")}</th>
                <th scope="col" class="num">{t("hub.reports.col.oldest")}</th>
              </tr>
            </thead>
            <tbody>
              {#each view.debtors as d (d.player_id)}
                <tr data-testid="debtor-row" data-erased={d.erased}>
                  <th scope="row" class:muted={d.erased}
                    >{who(d)}
                    {#if d.erased}<span class="muted type-hub-label"
                        >{t("hub.players.erased.id", { id: d.player_id })}</span
                      >{/if}</th
                  >
                  <td class="num tabular-nums">{formatMoney(d.outstanding_minor, locale)}</td>
                  <td class="num tabular-nums">{d.oldest_age_days}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </ScrollRegion>
      {/if}

      <ExportPanel report="players" selection={{ from: view.from, to: view.to }} />
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

  .note,
  .muted {
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
