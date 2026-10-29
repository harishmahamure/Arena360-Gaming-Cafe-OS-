<script lang="ts">
  import { Card, StatusBadge, formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { moneyLocale } from "./today.svelte";
  import type { ShiftRow } from "./reports";

  let { shifts, many }: { shifts: ShiftRow[]; many: boolean } = $props();

  const REASONS: Record<string, MessageKey> = {
    counting_error: "hub.shift.close.reason.counting_error",
    unrecorded_expense: "hub.shift.close.reason.unrecorded_expense",
    change_float_mismatch: "hub.shift.close.reason.change_float_mismatch",
    other: "hub.shift.close.reason.other",
  };

  const when = (ms: number, tz: string) => {
    const options = {
      day: "2-digit",
      month: "short",
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    } as const;
    try {
      return new Intl.DateTimeFormat("en-GB", { ...options, timeZone: tz }).format(ms);
    } catch {
      return new Intl.DateTimeFormat("en-GB", { ...options, timeZone: "UTC" }).format(ms);
    }
  };
  const signed = (v: number, currency: string) =>
    v === 0
      ? t("owner.today.variance.exact")
      : t(v > 0 ? "owner.today.variance.over" : "owner.today.variance.short", {
          amount: (v > 0 ? "+" : "") + formatMoney(v, moneyLocale(currency)),
        });
</script>

{#if shifts.length}
  <ul class="shifts">
    {#each shifts as s (s.branch.id + s.shift.shift_id)}
      {@const money = moneyLocale(s.currency)}
      {@const over = s.shift.over_threshold === true}
      <li>
        <Card>
          <dl class="shift" class:over>
            {#if many}
              <div>
                <dt>{t("owner.reports.cash.branch")}</dt>
                <dd>{s.branch.name}</dd>
              </div>
            {/if}
            <div>
              <dt>{t("owner.reports.cash.staff")}</dt>
              <dd>{s.shift.staff_name}</dd>
            </div>
            <div>
              <dt>{t("owner.reports.cash.opened")}</dt>
              <dd class="tabular-nums">{when(s.shift.opened_at, s.branch.timezone)}</dd>
            </div>
            <div>
              <dt>{t("owner.reports.cash.closed")}</dt>
              <dd class="tabular-nums">{when(s.shift.closed_at, s.branch.timezone)}</dd>
            </div>
            <div>
              <dt>{t("owner.reports.cash.float")}</dt>
              <dd class="tabular-nums">{formatMoney(s.shift.float, money)}</dd>
            </div>
            <div>
              <dt>{t("owner.reports.cash.expected")}</dt>
              <dd class="tabular-nums">{formatMoney(s.shift.expected, money)}</dd>
            </div>
            <div>
              <dt>{t("owner.reports.cash.counted")}</dt>
              <dd class="tabular-nums">{formatMoney(s.shift.counted, money)}</dd>
            </div>
            <div>
              <dt>{t("owner.reports.cash.variance")}</dt>
              <dd class="variance">
                <span class="tabular-nums">{signed(s.shift.variance, s.currency)}</span>
                {#if over}<StatusBadge
                    state="warning"
                    label="owner.today.variance.threshold"
                  />{/if}
              </dd>
            </div>
            <div>
              <dt>{t("owner.reports.cash.reason")}</dt>
              <dd>
                {s.shift.reason
                  ? t(REASONS[s.shift.reason] ?? "hub.shift.close.reason.other")
                  : t("owner.reports.cash.none")}
              </dd>
            </div>
            <div>
              <dt>{t("owner.reports.cash.approver")}</dt>
              <dd>{s.shift.approver_name ?? t("owner.reports.cash.none")}</dd>
            </div>
          </dl>
        </Card>
      </li>
    {/each}
  </ul>
{:else}
  <p>{t("owner.reports.cash.empty")}</p>
{/if}

<style>
  .shifts {
    /* The badge's tile colour is dark; on the paper theme it sits on the card instead. */
    --status-badge-background: var(--surface);
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-3);
  }
  .shift {
    margin: 0;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-3);
    padding-left: var(--space-3);
    border-left: 4px solid transparent;
  }
  .shift.over {
    border-left-color: var(--warning-text, var(--warning));
  }
  dt {
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .variance {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  @media (min-width: 768px) {
    .shift {
      grid-template-columns: repeat(5, minmax(0, 1fr));
    }
  }
</style>
