<script lang="ts">
  import { Card, StatusBadge, formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { age } from "./shell.svelte";
  import { moneyLocale } from "./today.svelte";
  import type { CurrencyView } from "./reports";

  let { views, many, now }: { views: CurrencyView[]; many: boolean; now: number } = $props();

  const TYPES = [
    ["play", "owner.reports.type.play"],
    ["products", "owner.reports.type.products"],
    ["plans", "owner.reports.type.plans"],
    ["other", "owner.reports.type.other"],
  ] as const satisfies readonly [string, MessageKey][];
  const METHODS = [
    ["cash", "owner.reports.method.cash"],
    ["upi", "owner.reports.method.upi"],
    ["card", "owner.reports.method.card"],
    ["wallet", "owner.reports.method.wallet"],
    ["credit", "owner.reports.method.credit"],
  ] as const satisfies readonly [string, MessageKey][];

  const ago = (ms: number) => {
    const [key, n] = age(new Date(ms).toISOString(), now);
    return t(key, { n });
  };
  const dates = (list: string[]) => list.join(", ");
</script>

{#each views as v (v.currency)}
  {@const money = moneyLocale(v.currency)}
  <Card>
    <section class="view" aria-labelledby="rev-{v.currency}">
      <h2 id="rev-{v.currency}" class="type-hub-title">
        {many
          ? t("owner.reports.currency", { currency: v.currency })
          : t("owner.reports.tab.revenue")}
      </h2>
      <div class="total">
        <span class="figure tabular-nums">{formatMoney(v.totals.revenue.total, money)}</span>
        <span class="type-hub-label">{t("owner.reports.revenue.total")}</span>
        <span class="type-hub-micro">{t("owner.reports.revenue.days", { n: v.totals.days })}</span>
        {#if v.partial}<StatusBadge state="warning" label="owner.reports.partial" />{/if}
      </div>
      {#each v.missing as m (m.branch)}
        <p class="muted type-hub-label">
          {#if v.missing.length > 1 || v.branches.length > 1}<strong>{m.branch}:</strong>{/if}
          {t(m.dates.length === 1 ? "owner.reports.missing.one" : "owner.reports.missing.many", {
            n: m.dates.length,
            dates: dates(m.dates),
          })}
        </p>
      {/each}

      <div class="groups">
        <div>
          <h3 class="type-hub-label">{t("owner.reports.revenue.by_type")}</h3>
          <dl>
            {#each TYPES as [key, label] (key)}
              <div class="line">
                <dt>{t(label)}</dt>
                <dd class="tabular-nums">{formatMoney(v.totals.revenue.by_type[key], money)}</dd>
              </div>
            {/each}
          </dl>
        </div>
        <div>
          <h3 class="type-hub-label">{t("owner.reports.revenue.by_method")}</h3>
          <dl>
            {#each METHODS as [key, label] (key)}
              <div class="line">
                <dt>{t(label)}</dt>
                <dd class="tabular-nums">{formatMoney(v.totals.revenue.by_method[key], money)}</dd>
              </div>
            {/each}
          </dl>
        </div>
        <div>
          <h3 class="type-hub-label">{t("owner.reports.revenue.adjustments")}</h3>
          <dl>
            <div class="line">
              <dt>{t("owner.reports.refunds", { n: v.totals.refunds.count })}</dt>
              <dd class="tabular-nums">{formatMoney(-v.totals.refunds.amount, money)}</dd>
            </div>
            <div class="line">
              <dt>{t("owner.reports.discounts", { n: v.totals.discounts.count })}</dt>
              <dd class="tabular-nums">{formatMoney(-v.totals.discounts.amount, money)}</dd>
            </div>
            <div class="line">
              <dt>{t("owner.reports.voids", { n: v.totals.voids.count })}</dt>
              <dd class="tabular-nums">{formatMoney(-v.totals.voids.amount, money)}</dd>
            </div>
          </dl>
        </div>
      </div>

      {#if v.branches.length > 1}
        <div>
          <h3 class="type-hub-label">{t("owner.reports.by_branch")}</h3>
          <dl>
            {#each v.branches as b (b.id)}
              <div class="line">
                <dt>{b.name}</dt>
                <dd class="tabular-nums">{formatMoney(b.minor, money)}</dd>
              </div>
            {/each}
          </dl>
        </div>
      {/if}

      {#if v.days.length}
        <div>
          <h3 class="type-hub-label">{t("owner.reports.by_day")}</h3>
          <dl>
            {#each v.days as d (d.day + d.live)}
              <div class="line" class:live={d.live}>
                <dt>
                  {d.day}
                  {#if d.live}
                    <span class="type-hub-micro"
                      >· {t("owner.reports.so_far")} · {t("owner.today.as_of", {
                        age: ago(d.asOf ?? now),
                      })}</span
                    >
                  {/if}
                </dt>
                <dd class="tabular-nums">{formatMoney(d.minor, money)}</dd>
              </div>
            {/each}
          </dl>
          {#if v.soFar.length}<p class="muted type-hub-micro">
              {t("owner.reports.so_far.note")}
            </p>{/if}
        </div>
      {:else}
        <p class="muted">{t("owner.reports.empty")}</p>
      {/if}
    </section>
  </Card>
{/each}

<style>
  .view {
    /* The badge's tile colour is dark; on the paper theme it sits on the card instead. */
    --status-badge-background: var(--surface);
    display: grid;
    gap: var(--space-4);
  }
  h2,
  h3,
  p {
    margin: 0;
  }
  h3 {
    margin-bottom: var(--space-2);
  }
  .total {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-3);
  }
  .figure {
    font-family: var(--type-hub-numeric-family);
    font-size: var(--type-hub-numeric-size);
    font-weight: var(--type-hub-numeric-weight);
    line-height: var(--type-hub-numeric-line-height);
  }
  .groups {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: var(--space-4);
  }
  dl {
    margin: 0;
    display: grid;
    gap: var(--space-1);
  }
  .line {
    display: flex;
    justify-content: space-between;
    gap: var(--space-3);
  }
  dt,
  dd {
    margin: 0;
  }
  .live dt {
    font-weight: var(--type-hub-title-weight);
  }
  .muted {
    color: var(--text-muted);
  }
  @media (min-width: 768px) {
    .groups {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }
</style>
