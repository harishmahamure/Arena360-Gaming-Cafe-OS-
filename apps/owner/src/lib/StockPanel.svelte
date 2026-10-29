<script lang="ts">
  import { Card, StatusBadge, formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { moneyLocale } from "./today.svelte";
  import type { StockView } from "./reports";

  let { views, many }: { views: StockView[]; many: boolean } = $props();

  const REASONS: Record<string, MessageKey> = {
    spoiled: "reason.spoiled",
    broken: "reason.broken",
    free: "reason.free",
    cancelled_order: "reason.cancelled_order",
    expired: "owner.reports.stock.waste.expired",
  };
  const reasonLabel = (r: string) => t(REASONS[r] ?? "reason.other");
  const dash = () => t("owner.reports.stock.missing");
</script>

{#each views as v (v.currency)}
  {@const money = moneyLocale(v.currency)}
  {@const cols = v.branches.length > 1}
  <Card>
    <section class="view" aria-labelledby="stk-{v.currency}">
      <h2 id="stk-{v.currency}" class="type-hub-title">
        {many
          ? t("owner.reports.currency", { currency: v.currency })
          : t("owner.reports.tab.stock")}
      </h2>

      {#each v.branches as b (b.id)}
        <p class="muted type-hub-label">
          {#if many}<strong>{b.name}:</strong>{/if}
          {#if b.levelsAsOf}
            {t("owner.reports.stock.as_of", { date: b.levelsAsOf })}
            {#if !b.tracked}· {t("owner.reports.stock.none")}{/if}
          {:else}
            {t("owner.reports.stock.no_data")}
          {/if}
        </p>
      {/each}

      <div>
        <h3 class="type-hub-label">{t("owner.reports.stock.live")}</h3>
        {#if v.live.length}
          <ul>
            {#each v.live as l (l.branch + l.product)}
              <li>
                {t("owner.today.attention.low_stock", {
                  branch: l.branch,
                  product: l.product,
                  qty: l.qty,
                })}
              </li>
            {/each}
          </ul>
        {:else}
          <p class="muted">{t("owner.reports.stock.live.none")}</p>
        {/if}
      </div>

      <div>
        <h3 class="type-hub-label">{t("owner.reports.stock.low")}</h3>
        {#if v.low.length}
          <ul>
            {#each v.low as l (l.branch + l.name)}
              <li>
                {t("owner.reports.stock.low.line", {
                  branch: l.branch,
                  product: l.name,
                  qty: l.qty,
                  low: l.low,
                })}
              </li>
            {/each}
          </ul>
        {:else}
          <p class="muted">{t("owner.reports.stock.low.none")}</p>
        {/if}
      </div>

      {#if v.products.length}
        <div>
          <h3 class="type-hub-label">{t("owner.reports.stock.levels")}</h3>
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrollable region must take focus) -->
          <div
            class="scroll"
            role="region"
            aria-label={t("owner.reports.stock.levels")}
            tabindex="0"
          >
            <table>
              <thead>
                <tr>
                  <th scope="col">{t("owner.reports.stock.col.product")}</th>
                  <th scope="col">{t("owner.reports.stock.col.category")}</th>
                  {#if cols}
                    {#each v.branches as b (b.id)}<th scope="col" class="num">{b.name}</th>{/each}
                    <th scope="col" class="num">{t("owner.reports.stock.col.total")}</th>
                  {:else}
                    <th scope="col" class="num">{t("owner.reports.stock.col.qty")}</th>
                    <th scope="col" class="num">{t("owner.reports.stock.col.low")}</th>
                  {/if}
                </tr>
              </thead>
              <tbody>
                {#each v.products as p (p.key)}
                  <tr>
                    <th scope="row">{p.name}</th>
                    <td>{p.category || dash()}</td>
                    {#if cols}
                      {#each v.branches as b (b.id)}
                        <td class="num tabular-nums">{p.cells[b.id]?.qty ?? dash()}</td>
                      {/each}
                      <td class="num tabular-nums">{p.total}</td>
                    {:else}
                      <td class="num tabular-nums">{p.total}</td>
                      <td class="num tabular-nums">
                        {p.cells[v.branches[0].id]?.low ?? dash()}
                      </td>
                    {/if}
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>
      {/if}

      <div>
        <h3 class="type-hub-label">{t("owner.reports.stock.waste")}</h3>
        {#if v.waste.length}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrollable region must take focus) -->
          <div
            class="scroll"
            role="region"
            aria-label={t("owner.reports.stock.waste")}
            tabindex="0"
          >
            <table>
              <thead>
                <tr>
                  <th scope="col">{t("owner.reports.stock.col.product")}</th>
                  <th scope="col">{t("owner.reports.stock.col.reason")}</th>
                  {#if cols}
                    {#each v.branches as b (b.id)}<th scope="col" class="num">{b.name}</th>{/each}
                    <th scope="col" class="num">{t("owner.reports.stock.col.total")}</th>
                  {:else}
                    <th scope="col" class="num">{t("owner.reports.stock.col.qty")}</th>
                    <th scope="col" class="num">{t("owner.reports.stock.col.value")}</th>
                  {/if}
                </tr>
              </thead>
              <tbody>
                {#each v.waste as w (w.key)}
                  <tr>
                    <th scope="row">{w.name}</th>
                    <td>{reasonLabel(w.reason)}</td>
                    {#if cols}
                      {#each v.branches as b (b.id)}
                        <td class="num tabular-nums">
                          {#if w.cells[b.id]}
                            {w.cells[b.id].qty} · {formatMoney(w.cells[b.id].value, money)}
                          {:else}{dash()}{/if}
                        </td>
                      {/each}
                      <td class="num tabular-nums">{w.qty} · {formatMoney(w.value, money)}</td>
                    {:else}
                      <td class="num tabular-nums">{w.qty}</td>
                      <td class="num tabular-nums">{formatMoney(w.value, money)}</td>
                    {/if}
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          <p class="muted type-hub-micro">{t("owner.reports.stock.waste.note")}</p>
        {:else}
          <p class="muted">{t("owner.reports.stock.waste.none")}</p>
        {/if}
      </div>

      <div>
        <h3 class="type-hub-label">{t("owner.reports.stock.expiring")}</h3>
        {#if v.expiring.length}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrollable region must take focus) -->
          <div
            class="scroll"
            role="region"
            aria-label={t("owner.reports.stock.expiring")}
            tabindex="0"
          >
            <table>
              <thead>
                <tr>
                  <th scope="col">{t("owner.reports.stock.col.product")}</th>
                  {#if many}<th scope="col">{t("owner.reports.stock.col.branch")}</th>{/if}
                  <th scope="col">{t("owner.reports.stock.col.lot")}</th>
                  <th scope="col" class="num">{t("owner.reports.stock.col.qty")}</th>
                  <th scope="col">{t("owner.reports.stock.col.expires")}</th>
                </tr>
              </thead>
              <tbody>
                {#each v.expiring as e (e.branch + e.name + e.lot)}
                  <tr>
                    <th scope="row">{e.name}</th>
                    {#if many}<td>{e.branch}</td>{/if}
                    <td>{e.lot}</td>
                    <td class="num tabular-nums">{e.qty}</td>
                    <td class="tabular-nums">
                      {e.expiresOn}
                      {#if e.expired}<StatusBadge
                          state="warning"
                          label="owner.reports.stock.expired"
                        />{/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <p class="muted">{t("owner.reports.stock.expiring.none")}</p>
        {/if}
      </div>
    </section>
  </Card>
{/each}

<style>
  .view {
    /* The badge's tile colour is dark; on the paper theme it sits on the card instead. */
    --status-badge-background: var(--surface);
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: var(--space-4);
  }
  h2,
  h3,
  p,
  ul {
    margin: 0;
  }
  h3 {
    margin-bottom: var(--space-2);
  }
  ul {
    padding-left: var(--space-4);
  }
  .muted {
    color: var(--text-muted);
  }
  .scroll {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th,
  td {
    padding: var(--space-1) var(--space-2);
    text-align: start;
    vertical-align: top;
    border-bottom: 1px solid var(--border-strong);
  }
  thead th {
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
  }
  tbody th {
    font-weight: var(--type-hub-title-weight);
  }
  .num {
    text-align: end;
  }
</style>
