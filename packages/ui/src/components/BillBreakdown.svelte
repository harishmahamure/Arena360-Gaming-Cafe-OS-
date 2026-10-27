<script lang="ts">
  import { formatMoney, formatTime } from "../format.js";
  import type { BillLine, SnapshotLocale } from "../generated";
  import { t } from "../messages/index.js";

  let {
    lines,
    subtotal,
    paid,
    remaining,
    locale,
    tz,
    names,
    group = false,
  }: {
    lines: BillLine[];
    subtotal: number;
    paid: number;
    remaining: number;
    locale: SnapshotLocale;
    tz: string | undefined;
    names: Record<string, string>;
    group?: boolean;
  } = $props();

  const money = (n: number) => formatMoney(n, locale);
  const perDevice = $derived.by(() => {
    if (!group) return [];
    const totals = new Map<string, number>();
    for (const l of lines) totals.set(l.device_id, (totals.get(l.device_id) ?? 0) + l.amount_minor);
    return totals.size > 1 ? [...totals] : [];
  });
</script>

<section class="bill" data-component="bill-breakdown" data-testid="bill-breakdown">
  <ul class="lines type-hub-label">
    {#each lines as l, i (i)}
      <li data-testid="bill-line">
        <span
          >{t(l.rule_name ? "hub.bill.line_rule" : "hub.bill.line", {
            device: names[l.device_id] ?? "",
            from: formatTime(l.from_at, tz),
            to: formatTime(l.to_at, tz),
            minutes: l.minutes,
            rate: money(l.rate_minor_per_hour),
            rule: l.rule_name ?? "",
          })}</span
        >
        <span class="amount tabular-nums">{money(l.amount_minor)}</span>
      </li>
    {/each}
  </ul>
  {#if perDevice.length > 0}
    <ul class="lines type-hub-label" data-testid="bill-device-subtotals">
      {#each perDevice as [device, amount] (device)}
        <li data-testid="bill-device-subtotal">
          <span>{t("hub.bill.device_subtotal", { device: names[device] ?? "" })}</span>
          <span class="amount tabular-nums">{money(amount)}</span>
        </li>
      {/each}
    </ul>
  {/if}
  <dl class="totals type-hub-label">
    <div data-testid="bill-subtotal">
      <dt>{t(group ? "hub.bill.group_total" : "hub.bill.subtotal")}</dt>
      <dd class="tabular-nums">{money(subtotal)}</dd>
    </div>
    <div data-testid="bill-paid">
      <dt>{t("hub.bill.paid")}</dt>
      <dd class="tabular-nums">{money(paid)}</dd>
    </div>
    <div data-testid="bill-remaining">
      <dt>{t("hub.bill.remaining")}</dt>
      <dd class="tabular-nums">{money(remaining)}</dd>
    </div>
  </dl>
</section>

<style>
  .bill {
    display: grid;
    gap: var(--space-3);
    width: 100%;
  }

  .lines,
  .totals {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li,
  .totals > div {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: var(--space-2);
  }

  dt {
    color: var(--text-muted);
  }

  dd {
    margin: 0;
  }

  .amount {
    margin-inline-start: auto;
  }
</style>
