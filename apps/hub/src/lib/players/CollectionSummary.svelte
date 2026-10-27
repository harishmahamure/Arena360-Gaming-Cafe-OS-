<script lang="ts">
  import { Button, formatDateTime, formatMoney, t } from "@arena/ui";
  import type { CreditCollection } from "@arena/ui/generated";
  import { branchLocale } from "../locale.svelte";

  let { collection, onclose }: { collection: CreditCollection; onclose: () => void } = $props();

  const locale = $derived(branchLocale.current);
  const money = (minor: number) => (locale ? formatMoney(minor, locale) : "");
  const shortRef = (id: string) => id.slice(-6).toUpperCase();
</script>

<section
  class="summary"
  aria-labelledby="collection-title"
  data-testid="collection-summary"
  tabindex="-1"
>
  <h3 class="type-hub-title" id="collection-title">{t("hub.credit.collect.summary.title")}</h3>
  <dl class="type-hub-body">
    <dt>{t("hub.credit.collect.summary.amount")}</dt>
    <dd class="tabular-nums" data-testid="collection-amount">{money(collection.amount_minor)}</dd>
    <dt>{t("hub.credit.collect.summary.after")}</dt>
    <dd class="tabular-nums" data-testid="collection-after">
      {money(collection.outstanding_after_minor)}
    </dd>
  </dl>
  <h4 class="type-hub-label">{t("hub.credit.collect.summary.cleared")}</h4>
  {#if collection.cleared.length === 0 || !locale}
    <p class="type-hub-body">{t("hub.credit.collect.summary.none")}</p>
  {:else}
    <ul class="type-hub-body" data-testid="collection-cleared">
      {#each collection.cleared as spend (spend.order_id)}
        <li class="tabular-nums" data-testid="collection-spend">
          {t("hub.credit.collect.summary.line", {
            date: formatDateTime(spend.at, locale.tz),
            ref: shortRef(spend.order_id),
            cleared: money(spend.cleared_minor),
            spend: money(spend.spend_minor),
          })}
        </li>
      {/each}
    </ul>
  {/if}
  <div class="actions">
    <Button label="hub.credit.collect.summary.close" variant="secondary" onclick={onclose} />
  </div>
</section>

<style>
  .summary {
    display: grid;
    gap: var(--space-3);
  }

  h3,
  h4,
  p {
    margin: 0;
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-1) var(--space-4);
    margin: 0;
  }

  dd {
    margin: 0;
  }

  ul {
    margin: 0;
    padding-left: var(--space-4);
    display: grid;
    gap: var(--space-1);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
</style>
