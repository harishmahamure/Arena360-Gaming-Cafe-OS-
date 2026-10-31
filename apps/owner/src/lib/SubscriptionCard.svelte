<script lang="ts">
  import { Button, Card, StatusBadge, formatDateTime, t, upgradeUrl } from "@arena/ui";
  import type { BranchPlan } from "@arena/ui/generated";
  import { PUBLIC_ONBOARDING_URL } from "$app/env/public";
  import type { MessageKey } from "@arena/ui/messages";
  import ChangePlan from "./ChangePlan.svelte";
  import SwitchCycle from "./SwitchCycle.svelte";
  import { checkoutOptions, loadRazorpay } from "./razorpay";
  import { shell } from "./shell.svelte";
  import {
    TIER_TEXT,
    addOnLines,
    changeMethod,
    changeOffer,
    keepPlan,
    keepable,
    methodOffer,
    unpaidDate,
    otherCycle,
    methodText,
    money,
    planName,
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
    onchanged,
  }: {
    plan: BranchPlan;
    cycle: Cycle;
    current?: boolean;
    /** Runs after a change is stored, withdrawn or found stale, so the page reloads the cards. */
    onchanged: (message: string) => void;
  } = $props();

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

  let changing = $state(false);
  let switching = $state(false);
  let methodBusy = $state(false);
  let methodFailed = $state<MessageKey | null>(null);
  let keeping = $state(false);
  let keepFailed = $state(false);
  const offer = $derived(changeOffer(plan));
  const unpaid = $derived(plan.state === "needs_approval" || plan.state === "grace");
  const unpaidAt = $derived(unpaidDate(plan));
  const pending = $derived(plan.pending_change);

  const pendingText = $derived.by(() => {
    if (!pending) return "";
    const cycle = pending.cycle ?? plan.cycle;
    const params = {
      plan: planName(pending.tier, cycle),
      cycle: t(
        cycle === "yearly" ? "owner.subscription.cycle.yearly" : "owner.subscription.cycle.monthly",
      ).toLowerCase(),
      date: day(pending.effective_at),
      amount: money(pending.renewal_total_minor, plan.currency ?? "INR"),
    };
    if (pending.subject === "method") return t("owner.subscription.pending.method");
    if (pending.needs_mandate) return t("owner.subscription.pending.mandate", params);
    if (pending.subject === "cycle") return t("owner.subscription.pending.cycle_later", params);
    return t(
      pending.kind === "now"
        ? "owner.subscription.pending.now"
        : "owner.subscription.pending.later",
      params,
    );
  });
  // A change that is being paid cannot be taken back; a scheduled or held one can, and a cycle
  // switch or new method only until the owner has authorised it.
  const canKeep = $derived(pending ? keepable(pending) : false);

  // "Change payment method": a Checkout that offers the methods the renewal amount allows. Closing
  // it, or a failed authorisation, leaves the previous method in force.
  async function changePaymentMethod() {
    methodBusy = true;
    methodFailed = null;
    const res = await changeMethod(plan);
    if (!res.ok || !res.data.checkout) {
      methodBusy = false;
      if (res.ok === false && res.code === "STALE") onchanged(t("owner.subscription.change.stale"));
      else methodFailed = "owner.subscription.method.failed";
      return;
    }
    const abandon = async () => {
      // Storing the change moved the subscription's `rev` on by one.
      const res = await keepPlan({ ...plan, rev: plan.rev + 1 });
      methodBusy = false;
      // A withdrawal that failed leaves the change held: reload the card to show it as it is.
      onchanged(res.ok ? t("owner.subscription.method.unchanged") : "");
    };
    try {
      const Razorpay = await loadRazorpay();
      const box = new Razorpay(
        checkoutOptions(res.data.checkout, t(TIER_TEXT[plan.tier]), {
          paid: () => {
            methodBusy = false;
            onchanged(t("owner.subscription.method.sent"));
          },
          dismissed: () => void abandon(),
        }),
      );
      // A failed attempt leaves Checkout open for a retry; closing it takes the change back.
      box.open();
    } catch {
      await abandon();
    }
  }

  async function keep() {
    keeping = true;
    keepFailed = false;
    const res = await keepPlan(plan);
    keeping = false;
    if (res.ok) onchanged(t("owner.subscription.pending.kept"));
    else if (res.code === "STALE") onchanged("");
    else keepFailed = true;
  }

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
        {#if unpaid}
          <p data-testid="unpaid-note">
            {t(
              plan.state === "grace"
                ? "owner.subscription.unpaid.grace"
                : "owner.subscription.unpaid.needs_approval",
              { date: unpaidAt === undefined ? "" : day(unpaidAt) },
            )}
          </p>
        {/if}
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
        {#if pending}
          <section class="pending" aria-labelledby="{headingId}-pending">
            <h3 id="{headingId}-pending" class="type-hub-title">
              {t("owner.subscription.pending.title")}
            </h3>
            <p data-testid="pending-change">{pendingText}</p>
            {#if keepFailed}<p role="alert">{t("owner.subscription.pending.keep_failed")}</p>{/if}
            {#if canKeep}
              <Button
                label="owner.subscription.pending.keep"
                variant="secondary"
                onclick={keep}
                disabled={keeping}
                disabledReason="owner.subscription.change.working"
              />
            {/if}
          </section>
        {/if}
        {#if changing}
          <ChangePlan
            {plan}
            oncancel={() => (changing = false)}
            ondone={(message) => {
              changing = false;
              onchanged(message);
            }}
            onstale={() => {
              changing = false;
              onchanged(t("owner.subscription.change.stale"));
            }}
          />
        {:else if switching}
          <SwitchCycle
            {plan}
            oncancel={() => (switching = false)}
            ondone={(message) => {
              switching = false;
              onchanged(message);
            }}
            onstale={() => {
              switching = false;
              onchanged(t("owner.subscription.change.stale"));
            }}
          />
        {:else if offer === "open"}
          <div class="actions">
            <Button
              label="owner.subscription.change.open"
              variant="secondary"
              onclick={() => (changing = true)}
            />
            <Button
              label={otherCycle(plan) === "yearly"
                ? "owner.subscription.cycle.open_yearly"
                : "owner.subscription.cycle.open_monthly"}
              variant="secondary"
              onclick={() => (switching = true)}
            />
            <Button
              label="owner.subscription.method.open"
              variant="secondary"
              onclick={changePaymentMethod}
              disabled={methodBusy}
              disabledReason="owner.subscription.change.working"
            />
          </div>
          {#if methodFailed}<p role="alert">{t(methodFailed)}</p>{/if}
        {:else if offer === "blocked"}
          <div class="actions">
            <Button
              label="owner.subscription.change.open"
              variant="secondary"
              disabled
              disabledReason="owner.subscription.change.blocked"
            />
            {#if methodOffer(plan)}
              <Button
                label="owner.subscription.method.open"
                variant="secondary"
                onclick={changePaymentMethod}
                disabled={methodBusy}
                disabledReason="owner.subscription.change.working"
              />
            {/if}
          </div>
          {#if methodFailed}<p role="alert">{t(methodFailed)}</p>{/if}
        {/if}
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
  h3,
  p {
    margin: 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .pending {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
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
