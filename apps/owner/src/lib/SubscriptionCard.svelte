<script lang="ts">
  import { Button, Card, StatusBadge, formatDateTime, t, upgradeUrl } from "@arena/ui";
  import type { BranchPlan } from "@arena/ui/generated";
  import { PUBLIC_ONBOARDING_URL } from "$app/env/public";
  import { shell } from "./shell.svelte";
  import {
    addOnLines,
    methodText,
    priceText,
    renewalPrice,
    stateText,
    toneOf,
    type Cycle,
  } from "./subscription";

  /** One branch's plan: tier, state, add-ons, renewal, payment method and the price of `cycle`. */
  let {
    plan,
    cycle,
    current = false,
  }: { plan: BranchPlan; cycle: Cycle; current?: boolean } = $props();

  const headingId = $props.id();
  const timezone = $derived(shell.branches?.find((b) => b.id === plan.branch_id)?.timezone);
  const day = (ms: number) => formatDateTime(ms, timezone, { dateStyle: "medium" });
  const isFree = $derived(plan.state === "free");
  const renews = $derived(plan.state !== "cancels" && plan.state !== "lapsed");
  const price = $derived(plan.prices.find((p) => p.cycle === cycle));
  const renewal = $derived(renewalPrice(plan));
  const addOns = $derived(addOnLines(plan));
  const TIERS = {
    free: "owner.tier.free",
    basic: "owner.tier.basic",
    premium: "owner.tier.premium",
  } as const;

  // Upgrade opens the same checkout the sign-up wizard uses, pre-set to this branch.
  const upgrade = () =>
    location.assign(upgradeUrl(PUBLIC_ONBOARDING_URL, "basic", plan.branch_id, location.href));
</script>

<li data-testid="subscription-{plan.branch_id}" aria-current={current ? "true" : undefined}>
  <Card>
    <section aria-labelledby={headingId} class="card">
      <div class="head">
        <h2 id={headingId} class="type-hub-title">{plan.branch_name}</h2>
        <span class="state">
          <StatusBadge
            tone={toneOf(plan.state)}
            text={stateText(
              plan.state,
              plan.current_period_end ? day(plan.current_period_end) : "",
            )}
          />
        </span>
      </div>
      {#if isFree}
        <p>{t("owner.subscription.free_line")}</p>
        <Button label="owner.subscription.upgrade" onclick={upgrade} />
      {:else}
        {#if plan.message}<p>{plan.message}</p>{/if}
        <dl class="facts">
          <div>
            <dt>{t("owner.subscription.tier")}</dt>
            <dd>
              {t(TIERS[plan.tier])} ·
              {t(
                plan.cycle === "yearly"
                  ? "owner.subscription.cycle.yearly"
                  : "owner.subscription.cycle.monthly",
              )}
            </dd>
          </div>
          <div>
            <dt>{t("owner.subscription.addons")}</dt>
            <dd>{addOns.length ? addOns.join(", ") : t("owner.subscription.no_addons")}</dd>
          </div>
          <div>
            <dt>{t("owner.subscription.price")}</dt>
            <dd class="tabular-nums">
              {price ? priceText(price, plan) : t("owner.subscription.price.none")}
            </dd>
          </div>
          {#if plan.current_period_end && (renews || plan.state === "cancels")}
            <div>
              <dt>{t(renews ? "owner.subscription.renews" : "owner.subscription.ends")}</dt>
              <dd class="tabular-nums">
                {#if renews && renewal}
                  {t("owner.subscription.renews_value", {
                    date: day(plan.current_period_end),
                    amount: priceText(renewal, plan),
                  })}
                {:else}
                  {day(plan.current_period_end)}
                {/if}
              </dd>
            </div>
          {/if}
          <div>
            <dt>{t("owner.subscription.payment")}</dt>
            <dd>{methodText(plan.payment_method)}</dd>
          </div>
        </dl>
      {/if}
    </section>
  </Card>
</li>

<style>
  li {
    list-style: none;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  /* The badge is drawn for the Hub tiles; on a card it sits on the card surface. */
  .state {
    --status-badge-background: var(--card-background);
    display: inline-flex;
    max-width: 100%;
  }
  h2,
  p {
    margin: 0;
  }
  .facts {
    display: grid;
    gap: var(--space-2);
    margin: 0;
  }
  .facts div {
    display: grid;
    gap: var(--space-1);
  }
  dt {
    color: var(--text-muted);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
