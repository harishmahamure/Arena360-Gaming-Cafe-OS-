<script lang="ts">
  import { Button, formatDateTime, formatMoney, t } from "@arena/ui";
  import type {
    ControllerResult,
    SessionListRow,
    SessionListState,
    SessionListView,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { goto, invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, loadSessions } from "../../lib/auth";
  import { reprintReceipt } from "../../lib/pos-sales";
  import type { PageData } from "./$types";

  let { data }: { data: PageData } = $props();

  type Filter = "all" | SessionListState;

  let reprintNote: { text: MessageKey; tone: "ok" | "refusal" } | null = $state(null);

  /** Prints the receipt of a settled bill again (Story 5.17); the Hub marks it "Reprint". */
  async function reprint(row: SessionListRow) {
    if (!row.receipt_order_id) return;
    const outcome = await reprintReceipt(row.receipt_order_id);
    if (outcome === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    reprintNote =
      outcome === "queued"
        ? { tone: "ok", text: "hub.pos.recent.reprint_queued" }
        : { tone: "refusal", text: "hub.pos.recent.reprint_failed" };
  }

  const FILTERS: { value: Filter; label: MessageKey }[] = [
    { value: "all", label: "hub.sessions.filter.all" },
    { value: "running", label: "hub.sessions.filter.running" },
    { value: "unpaid", label: "hub.sessions.filter.unpaid" },
    { value: "settled", label: "hub.sessions.filter.settled" },
  ];
  const STATE: Record<SessionListState, MessageKey> = {
    running: "hub.sessions.state.running",
    unpaid: "hub.sessions.state.unpaid",
    settled: "hub.sessions.state.settled",
  };
  const CONTROLLERS: Record<ControllerResult, MessageKey> = {
    returned: "hub.sessions.controllers.returned",
    missing: "hub.sessions.controllers.missing",
  };
  const MODE: Record<string, MessageKey> = {
    postpaid: "hub.sessions.mode.postpaid",
    pack: "hub.sessions.mode.pack",
    wallet: "hub.sessions.mode.wallet",
    pass: "hub.sessions.mode.pass",
    block: "hub.sessions.mode.block",
  };

  const snapshot = $derived(data.hub.ok ? data.hub.snapshot : null);
  const locale = $derived(snapshot?.locale ?? null);
  const tz = $derived(locale?.tz);

  let view = $state<SessionListView | null>(null);
  let failed = $state(false);
  let filter: Filter = $state("all");
  let turn = 0;

  async function refresh() {
    const mine = ++turn;
    const outcome = await loadSessions();
    if (mine !== turn) return;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    failed = outcome.kind !== "ok";
    if (outcome.kind === "ok") view = outcome.value;
  }

  $effect(() => {
    void snapshot;
    void refresh();
  });

  const rows = $derived(
    (view?.sessions ?? []).filter((r) => filter === "all" || r.state === filter),
  );

  const who = (r: SessionListRow) => r.username ?? t("hub.session.guest", { n: r.guest_no ?? 0 });

  const bill = (r: SessionListRow): number | null =>
    r.bill_minor ?? snapshot?.bills?.find((b) => b.session_id === r.id)?.subtotal_minor ?? null;

  const open = (r: SessionListRow) => void goto(`/floor?device=${encodeURIComponent(r.device_id)}`);
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.nav.sessions") })}</title>
</svelte:head>

<section class="sessions" data-testid="sessions-screen">
  <h1 class="type-hub-numeric">{t("hub.nav.sessions")}</h1>
  {#if view?.business_day}
    <p class="type-hub-label muted" data-testid="sessions-day">
      {t("hub.sessions.day", { day: view.business_day })}
    </p>
  {/if}

  <div class="filter">
    <label class="type-hub-label" for="sessions-filter">{t("hub.sessions.filter.label")}</label>
    <select id="sessions-filter" class="ui-interactive select" bind:value={filter}>
      {#each FILTERS as f (f.value)}
        <option value={f.value}>{t(f.label)}</option>
      {/each}
    </select>
  </div>

  <div role="status">
    {#if failed}
      <p class="type-hub-label warn">
        <span aria-hidden="true">⚠</span>
        {t("hub.sessions.failed")}
      </p>
    {:else if view && rows.length === 0}
      <p class="type-hub-body muted" data-testid="sessions-empty">{t("hub.sessions.empty")}</p>
    {/if}
  </div>

  {#if reprintNote}
    <p class="type-hub-label" role="status" data-testid="reprint-note">
      {#if reprintNote.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      {t(reprintNote.text)}
    </p>
  {/if}

  {#if rows.length > 0 && locale}
    <div class="scroll">
      <table class="type-hub-body" data-testid="sessions-table">
        <caption class="sr-only">{t("hub.sessions.caption")}</caption>
        <thead>
          <tr>
            <th scope="col">{t("hub.sessions.col.device")}</th>
            <th scope="col">{t("hub.sessions.col.player")}</th>
            <th scope="col">{t("hub.sessions.col.mode")}</th>
            <th scope="col">{t("hub.sessions.col.start")}</th>
            <th scope="col">{t("hub.sessions.col.end")}</th>
            <th scope="col" class="num">{t("hub.sessions.col.bill")}</th>
            <th scope="col">{t("hub.sessions.col.state")}</th>
            <th scope="col">{t("hub.sessions.col.controllers")}</th>
            <th scope="col"><span class="sr-only">{t("hub.sessions.col.action")}</span></th>
          </tr>
        </thead>
        <tbody>
          {#each rows as r (r.id)}
            {@const amount = bill(r)}
            <tr data-testid="session-row" data-state={r.state}>
              <td>{r.device}</td>
              <td data-testid="session-player">{who(r)}</td>
              <td>{t(MODE[r.mode] ?? "hub.sessions.mode.postpaid")}</td>
              <td class="tabular-nums">{formatDateTime(r.started_at, tz)}</td>
              <td class="tabular-nums">{r.ended_at ? formatDateTime(r.ended_at, tz) : "—"}</td>
              <td class="num tabular-nums">{amount === null ? "—" : formatMoney(amount, locale)}</td
              >
              <td data-testid="session-state"
                ><span aria-hidden="true"
                  >{r.state === "settled" ? "✓" : r.state === "unpaid" ? "⚠" : "●"}</span
                >
                {t(STATE[r.state])}</td
              >
              <td data-testid="session-controllers">
                {r.controller_check ? t(CONTROLLERS[r.controller_check]) : "—"}
              </td>
              <td>
                {#if r.state === "unpaid"}
                  <Button label="hub.sessions.open" variant="secondary" onclick={() => open(r)} />
                {:else if r.receipt_order_id}
                  <Button
                    label="hub.sessions.reprint"
                    variant="secondary"
                    onclick={() => void reprint(r)}
                  />
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .sessions {
    display: grid;
    gap: var(--space-3);
    align-content: start;
  }

  h1,
  p {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .warn {
    color: var(--text-field-error-text);
  }

  .filter {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }

  .select {
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
    font: inherit;
  }

  .scroll {
    overflow-x: auto;
  }

  table {
    inline-size: 100%;
    border-collapse: collapse;
  }

  th,
  td {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border-strong);
    text-align: start;
    vertical-align: middle;
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
