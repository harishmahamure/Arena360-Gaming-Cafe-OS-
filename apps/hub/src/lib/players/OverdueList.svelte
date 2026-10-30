<script lang="ts">
  import { formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { OverdueRow, OverdueView } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, loadOverdue } from "../auth";
  import { branchLocale } from "../locale.svelte";

  let { onpick }: { onpick: (playerId: string) => void } = $props();

  const BUCKET: Record<string, MessageKey> = {
    "0_7": "hub.credit.aging.b0",
    "8_14": "hub.credit.aging.b8",
    "15_plus": "hub.credit.aging.b15",
  };

  let view = $state<OverdueView | null>(null);
  let failed = $state(false);
  let loading = $state(true);

  const locale = $derived(branchLocale.current);
  const money = (minor: number) => (locale ? formatMoney(minor, locale) : "");

  function consent(row: OverdueRow): MessageKey {
    if (row.consent === true) return "hub.players.overdue.consent.granted";
    if (row.consent === false) return "hub.players.overdue.consent.withdrawn";
    return "hub.players.overdue.consent.none";
  }

  onMount(async () => {
    const outcome = await loadOverdue();
    loading = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    failed = outcome.kind !== "ok";
    if (outcome.kind === "ok") view = outcome.value;
  });
</script>

<section class="overdue" aria-labelledby="overdue-title" data-testid="overdue">
  <h2 class="type-hub-title" id="overdue-title">{t("hub.players.overdue.title")}</h2>
  <p class="type-hub-body muted">{t("hub.players.overdue.intro")}</p>

  {#if view && !view.reminders}
    <div class="tier-lock" data-testid="overdue-lock">
      <p class="lock type-hub-label">
        <span aria-hidden="true">🔒</span>
        {t("hub.players.overdue.locked")}
      </p>
      <p class="type-hub-label">{t("hub.players.overdue.locked_note")}</p>
    </div>
  {/if}

  <div role="status" class="type-hub-label">
    {#if loading}{t("hub.players.overdue.loading")}{/if}
    {#if failed}<span aria-hidden="true">⚠</span> {t("hub.players.overdue.failed")}{/if}
    {#if view && view.rows.length === 0}{t("hub.players.overdue.empty")}{/if}
  </div>

  {#if view && view.rows.length > 0}
    <ul class="rows" data-testid="overdue-list">
      {#each view.rows as row (row.player_id)}
        <li>
          <button
            type="button"
            class="ui-interactive row type-hub-body"
            data-testid="overdue-row"
            onclick={() => onpick(row.player_id)}
          >
            <span class="name"
              >{row.erased ? t("hub.players.overdue.erased") : row.name}
              {#if row.erased}<span class="type-hub-label"
                  >{t("hub.players.erased.id", { id: row.player_id })}</span
                >{/if}</span
            >
            <span class="amount tabular-nums" data-testid="overdue-amount">
              {money(row.outstanding_minor)}
            </span>
            <span class="type-hub-label">
              {t(BUCKET[row.bucket] ?? "hub.credit.aging.b0")} ·
              {t("hub.players.overdue.oldest", {
                day: row.oldest_unpaid_day,
                days: String(row.age_days),
              })}
            </span>
            <span class="type-hub-label" data-testid="overdue-consent">{t(consent(row))}</span>
            {#if view.reminders}
              <span class="type-hub-label" data-testid="overdue-last">
                {row.last_reminder_day
                  ? t("hub.players.overdue.last", { day: row.last_reminder_day })
                  : t("hub.players.overdue.never")}
              </span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .overdue {
    display: grid;
    gap: var(--space-2);
  }

  h2,
  p {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .tier-lock {
    display: grid;
    gap: var(--space-1);
    color: var(--tier-lock-foreground);
  }

  .lock {
    color: var(--text);
  }

  .rows {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    --ui-bg: var(--button-secondary-background);
    display: grid;
    grid-template-columns: 1fr auto;
    gap: var(--space-1) var(--space-4);
    width: 100%;
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    color: var(--button-secondary-foreground);
    font: inherit;
    text-align: start;
    cursor: pointer;
  }

  .row > .type-hub-label {
    grid-column: 1 / -1;
  }

  .amount {
    text-align: end;
  }
</style>
