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
  }: {
    lines: BillLine[];
    subtotal: number;
    paid: number;
    remaining: number;
    locale: SnapshotLocale;
    tz: string | undefined;
    names: Record<string, string>;
  } = $props();

  const money = (n: number) => formatMoney(n, locale);
</script>

<section class="bill" data-component="bill-breakdown" data-testid="bill-breakdown">
  <ul class="lines type-hub-label">
    {#each lines as l, i (i)}
      <li data-testid="bill-line">
        <span
          >{t("hub.bill.line", {
            device: names[l.device_id] ?? "",
            from: formatTime(l.from_at, tz),
            to: formatTime(l.to_at, tz),
            minutes: l.minutes,
            rate: money(l.rate_minor_per_hour),
          })}</span
        >
        <span class="amount tabular-nums">{money(l.amount_minor)}</span>
      </li>
    {/each}
  </ul>
  <dl class="totals type-hub-label">
    <div data-testid="bill-subtotal">
      <dt>{t("hub.bill.subtotal")}</dt>
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
