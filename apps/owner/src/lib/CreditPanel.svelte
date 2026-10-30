<script lang="ts">
  import { Button, Card, formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { RollupCreditPlayer } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import CommandRow from "./CommandRow.svelte";
  import {
    POLL_MS,
    cmd,
    groupOf,
    localRows,
    refreshCommands,
    sendNow,
    type Row,
  } from "./commands.svelte";
  import type { CreditBranch } from "./reports";
  import { moneyLocale } from "./today.svelte";

  let { branches, many, now }: { branches: CreditBranch[]; many: boolean; now: number } = $props();

  const BUCKETS = [
    ["d0_7", "owner.reports.credit.bucket.0_7"],
    ["d8_14", "owner.reports.credit.bucket.8_14"],
    ["d15p", "owner.reports.credit.bucket.15_plus"],
  ] as const satisfies readonly [string, MessageKey][];

  const KIND = "credit.remind";
  const slot = (branchId: string, playerId: string) => `${branchId}:${playerId}`; // i18n-ignore

  // The reminders the owner has sent (or queued on this device), newest first.
  const rows = $derived([...localRows(), ...cmd.rows]);
  const playerOf = (r: Row) => (r.payload as { player_id?: unknown } | null)?.player_id;
  const lastRow = (branchId: string, playerId: string): Row | undefined =>
    rows
      .filter((r) => r.kind === KIND && r.branch_id === branchId && playerOf(r) === playerId)
      .sort((a, b) => b.queued_at - a.queued_at)[0];

  // Reminded today: the Hub's list (automatic or manual), or one the owner just sent that the
  // Hub has not refused. The Hub's own summary catches up within a cadence.
  function reminded(b: CreditBranch, playerId: string) {
    if (b.reminded.includes(playerId)) return true;
    const last = lastRow(b.id, playerId);
    return (
      !!last &&
      last.status !== "rejected" &&
      last.status !== "expired" &&
      groupOf(last, now) === "today"
    );
  }

  let confirming: string | null = $state(null);
  let sending: Record<string, boolean> = $state({});
  let failed: Record<string, boolean> = $state({});

  async function remind(b: CreditBranch, p: RollupCreditPlayer) {
    const k = slot(b.id, p.player_id);
    if (sending[k]) return;
    confirming = null;
    sending[k] = true;
    failed[k] = false;
    const res = await sendNow(b.id, KIND, { player_id: p.player_id });
    sending[k] = false;
    failed[k] = res === "failed";
  }

  // One tap sends; a player already reminded today asks to confirm first.
  function tap(b: CreditBranch, p: RollupCreditPlayer) {
    if (reminded(b, p.player_id)) confirming = slot(b.id, p.player_id);
    else void remind(b, p);
  }

  onMount(() => {
    const tick = () => {
      if (document.visibilityState === "visible") void refreshCommands();
    };
    tick();
    const timer = setInterval(tick, POLL_MS);
    document.addEventListener("visibilitychange", tick);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", tick);
    };
  });
</script>

{#each branches as b (b.id)}
  {@const money = moneyLocale(b.currency)}
  <Card>
    <section class="view" aria-labelledby="crd-{b.id}">
      <h2 id="crd-{b.id}" class="type-hub-title">
        {many ? b.name : t("owner.reports.tab.credit")}
      </h2>
      {#if !b.asOf}
        <p class="muted">{t("owner.reports.credit.none")}</p>
      {:else}
        <p class="muted type-hub-label">{t("owner.reports.credit.as_of", { date: b.asOf })}</p>
        <p>
          <span class="type-hub-label">{t("owner.reports.credit.outstanding")}</span>
          <strong class="figure tabular-nums">{formatMoney(b.outstanding, money)}</strong>
        </p>

        <div>
          <h3 class="type-hub-label">{t("owner.reports.credit.aged")}</h3>
          <dl class="aged">
            {#each BUCKETS as [key, label] (key)}
              <div>
                <dt class="muted">{t(label)}</dt>
                <dd class="tabular-nums">{formatMoney(b.aged[key], money)}</dd>
              </div>
            {/each}
          </dl>
        </div>

        <div>
          <h3 class="type-hub-label">{t("owner.reports.credit.players")}</h3>
          {#if b.players.length}
            <ul class="players">
              {#each b.players as p (p.player_id)}
                {@const k = slot(b.id, p.player_id)}
                {@const last = lastRow(b.id, p.player_id)}
                <li class="player" data-player={p.player_id}>
                  <div class="who">
                    <span class="type-hub-label">{p.username || p.player_id}</span>
                    <span class="muted type-hub-micro">{p.player_id}</span>
                  </div>
                  <div class="owes">
                    <span class="tabular-nums">{formatMoney(p.outstanding, money)}</span>
                    {#if p.oldest_unpaid_on}
                      <span class="muted type-hub-micro tabular-nums">
                        {t("owner.reports.credit.oldest", { date: p.oldest_unpaid_on })}
                      </span>
                    {/if}
                  </div>
                  <div
                    class="actions"
                    role="group"
                    aria-label={t("owner.reports.credit.player", {
                      username: p.username || p.player_id,
                      id: p.player_id,
                    })}
                  >
                    {#if !p.consent}
                      <span class="muted">{t("owner.reports.credit.no_consent")}</span>
                    {:else if confirming === k}
                      <span>{t("owner.reports.credit.reminded")}</span>
                      <Button
                        label="owner.reports.credit.again"
                        variant="warning"
                        onclick={() => remind(b, p)}
                      />
                      <Button
                        label="owner.cmd.cancel"
                        variant="secondary"
                        onclick={() => (confirming = null)}
                      />
                    {:else}
                      {#if reminded(b, p.player_id)}
                        <span class="muted">{t("owner.reports.credit.reminded")}</span>
                      {/if}
                      <Button
                        label="owner.reports.credit.remind"
                        variant="secondary"
                        onclick={() => tap(b, p)}
                        disabled={!!sending[k]}
                        disabledReason="owner.reports.credit.sending"
                      />
                    {/if}
                  </div>
                  {#if failed[k]}
                    <p class="failed type-hub-micro" role="alert">
                      {t("owner.reports.credit.send.failed")}
                    </p>
                  {/if}
                  {#if p.consent && last}
                    <ul class="rows"><CommandRow row={last} {now} /></ul>
                  {/if}
                </li>
              {/each}
            </ul>
          {:else}
            <p class="muted">{t("owner.reports.credit.players.none")}</p>
          {/if}
        </div>
      {/if}
    </section>
  </Card>
{/each}

<style>
  .view {
    --status-badge-background: var(--surface);
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: var(--space-4);
  }
  h2,
  h3,
  p,
  dl,
  dd {
    margin: 0;
  }
  h3 {
    margin-bottom: var(--space-2);
  }
  .muted {
    color: var(--text-muted);
  }
  .figure {
    margin-inline-start: var(--space-2);
  }
  .aged {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-5);
  }
  .players,
  .rows {
    display: grid;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .player {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--border-strong);
  }
  .who,
  .owes {
    display: grid;
    gap: var(--space-1);
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
  .failed {
    flex-basis: 100%;
    color: var(--danger);
  }
  .rows {
    flex-basis: 100%;
  }
  @media print {
    .actions,
    .rows {
      display: none;
    }
  }
</style>
