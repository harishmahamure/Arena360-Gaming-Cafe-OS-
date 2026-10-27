<script lang="ts">
  import { formatMoney, t } from "@arena/ui";
  import type { CreditAging, CreditView } from "@arena/ui/generated";
  import { untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, loadWallet } from "../auth";
  import { branchLocale } from "../locale.svelte";
  import { walletEvents } from "../wallet.svelte";

  let { playerId }: { playerId: string } = $props();

  const locale = $derived(branchLocale.current);
  let credit = $state<CreditView | null>(null);
  let aging = $state<CreditAging | null>(null);
  let turn = 0;

  const money = (minor: number) => (locale ? formatMoney(minor, locale) : "");
  const shown = $derived(credit !== null && (credit.outstanding_minor > 0 || credit.paused));

  async function refresh() {
    const mine = ++turn;
    const outcome = await loadWallet(playerId);
    if (mine !== turn) return;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "ok") {
      credit = outcome.value.credit ?? null;
      aging = outcome.value.aging ?? null;
    }
  }

  $effect(() => {
    void playerId;
    untrack(() => void refresh());
  });

  $effect(() => {
    const changed = walletEvents.version;
    if (changed > 0 && walletEvents.playerId === untrack(() => playerId)) {
      untrack(() => void refresh());
    }
  });
</script>

{#if shown && credit}
  <section class="outstanding" aria-labelledby="outstanding-title" data-testid="outstanding">
    <h2 class="type-hub-title" id="outstanding-title">{t("hub.credit.aging.title")}</h2>
    <p class="total type-hub-numeric tabular-nums" data-testid="outstanding-total">
      {money(credit.outstanding_minor)}
    </p>
    {#if credit.paused}
      <p class="type-hub-label" role="status" data-testid="credit-paused">
        <span aria-hidden="true">⚠</span>
        {t("hub.credit.paused")}
      </p>
    {/if}
    {#if aging}
      <dl class="type-hub-body" data-testid="aging">
        <dt>{t("hub.credit.aging.b0")}</dt>
        <dd class="tabular-nums" data-testid="aging-0-7">{money(aging.bucket_0_7_minor)}</dd>
        <dt>{t("hub.credit.aging.b8")}</dt>
        <dd class="tabular-nums" data-testid="aging-8-14">{money(aging.bucket_8_14_minor)}</dd>
        <dt>{t("hub.credit.aging.b15")}</dt>
        <dd class="tabular-nums" data-testid="aging-15-plus">
          {money(aging.bucket_15_plus_minor)}
        </dd>
        <dt>{t("hub.credit.aging.oldest")}</dt>
        <dd class="tabular-nums" data-testid="aging-oldest">
          {aging.oldest_unpaid_day ?? t("hub.credit.aging.none")}
        </dd>
      </dl>
    {/if}
  </section>
{/if}

<style>
  .outstanding {
    display: grid;
    gap: var(--space-2);
  }

  h2,
  p {
    margin: 0;
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-1) var(--space-4);
    margin: 0;
  }

  dt {
    color: var(--text-muted);
  }

  dd {
    margin: 0;
  }
</style>
