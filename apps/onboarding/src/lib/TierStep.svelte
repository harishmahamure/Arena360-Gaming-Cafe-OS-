<script lang="ts">
  import { Button, PRICING, TabControl, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type {
    AddOns,
    Band,
    SubscriptionQuote,
    Tier,
    TierChoices,
    TierUnavailable,
  } from "@arena/ui/generated";
  import { goto } from "$app/navigation";
  import { onDestroy, onMount, tick } from "svelte";
  import {
    chooseTier,
    createSubscription,
    getTierChoices,
    getWizard,
    quoteSubscription,
    type ApiFailure,
  } from "./api";
  import AddOnsPanel from "./AddOnsPanel.svelte";
  import CheckoutSummary from "./CheckoutSummary.svelte";
  import { CONFIRM_POLL_MS, CONFIRM_SLOW_MS, checkoutOptions, loadRazorpay } from "./razorpay";
  import { money, snapshotPrice, type Cycle } from "./tierPrice";
  import { STEP_NAMES, advance, setWizard, stepPath, wizard } from "./wizard.svelte";

  /** A limit a paid tier does not cap (`arena_proto::UNLIMITED`, `u32::MAX`). */
  const UNLIMITED = 4_294_967_295;

  type Plan = (typeof PRICING)[number];
  type Phase = "pick" | "summary" | "confirming";
  type Period = Cycle;
  /**
   * Upgrade mode (Story 10.5): the same picker and Checkout for a branch whose wizard is over or
   * running elsewhere. `tier` is what the lock named; `onDone` runs once the entitlement is written.
   */
  type Upgrade = {
    tier: Tier | null;
    onDone: () => void;
    onExists: () => void;
    onBack: () => void;
  };

  let { upgrade = null }: { upgrade?: Upgrade | null } = $props();

  const id = $props.id();
  const PERIODS: { id: Period; label: MessageKey }[] = [
    { id: "monthly", label: "onboarding.tier.monthly" },
    { id: "yearly", label: "onboarding.tier.yearly" },
  ];
  const REASONS: Record<TierUnavailable, MessageKey> = {
    one_free_branch: "onboarding.tier.reason.one_free_branch",
    billing_disabled: "onboarding.tier.reason.billing_disabled",
    band_not_available: "pricing.band_unavailable",
  };

  let period: string = $state("monthly");
  let choices: TierChoices | null = $state(null);
  let loadError: MessageKey | null = $state(null);
  let selected: Tier | null = $state(null);
  let saving = $state(false);
  let formError: MessageKey | null = $state(null);
  let phase: Phase = $state("pick");
  let addOns: AddOns = $state({ devices_20: 0, tunnel: 0, owner_login: 0 });
  let quote: SubscriptionQuote | null = $state(null);
  let paying = $state(false);
  let declined = $state(false);
  let payError: MessageKey | null = $state(null);
  let slow = $state(false);
  let summaryHeading: HTMLElement | null = $state(null);
  let leaving = false;

  const plans = $derived(upgrade ? PRICING.filter((p) => p.key !== "free") : PRICING);
  const optionOf = (key: Tier) => choices?.tiers.find((o) => o.key === key) ?? null;
  const canContinue = $derived(selected !== null && optionOf(selected)?.available === true);
  const cycle: Cycle = $derived(period === "yearly" ? "yearly" : "monthly");
  const band: Band = $derived.by(() => choices?.band ?? "IN");
  // A function body, so the flow analysis does not narrow `choices` to its initial `null`.
  const billingOn = (): boolean => choices?.billing_enabled === true;
  const buyable = $derived(billingOn());
  /** A paid plan is picked and can be bought: Continue goes to the summary, not to the API. */
  const buying = $derived.by(
    () =>
      billingOn() &&
      (selected === "basic" || selected === "premium") &&
      optionOf(selected)?.available === true,
  );

  function failure(f: ApiFailure): MessageKey {
    return f.offline ? "onboarding.error.offline" : "onboarding.error.generic";
  }

  /** The wizard moved on, or (upgrade mode) the branch already has a subscription. */
  async function leaveForCurrentStep(code?: string): Promise<boolean> {
    if (upgrade) {
      if (code !== "SUBSCRIPTION_EXISTS") return false;
      upgrade.onExists();
      return true;
    }
    const fresh = await getWizard();
    if (!fresh.ok) return false;
    setWizard(fresh.data);
    if (fresh.data.current === "tier") return false;
    await goto(stepPath(fresh.data.current), { replaceState: true });
    return true;
  }

  async function load() {
    loadError = null;
    const res = await getTierChoices();
    if (!res.ok) {
      selected = null;
      choices = null;
      if (res.error.code === "FORBIDDEN" && (await leaveForCurrentStep("FORBIDDEN"))) return;
      loadError =
        upgrade && res.error.code === "FORBIDDEN" ? "onboarding.upgrade.forbidden" : failure(res);
      return;
    }
    choices = res.data;
    const current = res.data.current;
    const usable = (key: Tier | null | undefined) =>
      key ? res.data.tiers.some((o) => o.key === key && o.available) : false;
    if (upgrade) {
      const paid = (key: Tier | null | undefined) => key !== "free" && usable(key);
      if (!paid(selected)) {
        selected = paid(upgrade.tier) ? upgrade.tier : paid("basic") ? "basic" : null;
      }
    } else if (usable(current)) selected = current ?? null;
    else if (!usable(selected)) selected = res.data.tiers.find((o) => o.available)?.key ?? null;
  }

  onMount(load);

  function describedBy(inputId: string, hasReason: boolean): string {
    const ids = [inputId + "-price"];
    if (hasReason) ids.push(inputId + "-reason");
    return ids.join(" ");
  }

  function price(plan: Plan): string {
    const line = plan.key === "free" ? null : snapshotPrice(band, plan.key, cycle);
    // A paid plan with no price line (or, with billing on, no band) shows no price rather than a
    // made-up amount.
    if (plan.key !== "free" && (!line || (buyable && !choices?.band))) return "";
    const currency = line?.currency ?? snapshotPrice(band, "basic", cycle)?.currency ?? "INR";
    const amount = money(line?.minor ?? 0, currency);
    const key: MessageKey =
      period === "yearly"
        ? plan.perBranch
          ? "pricing.price.branch_year"
          : "pricing.price.year"
        : plan.perBranch
          ? "pricing.price.branch_month"
          : "pricing.price.month";
    return t(key, { price: amount });
  }

  /** GST shows on band IN only, and only on a priced plan. */
  const taxed = (plan: Plan): boolean =>
    plan.key !== "free" &&
    !(buyable && !choices?.band) &&
    snapshotPrice(band, plan.key, cycle)?.gst === true;

  function pickPlan(key: Tier) {
    selected = key;
    if (key !== "basic") addOns.tunnel = 0;
  }

  async function focus(el: () => HTMLElement | null) {
    await tick();
    el()?.focus();
  }

  async function review() {
    const branchId = wizard.state?.branch?.id;
    if (!branchId || (selected !== "basic" && selected !== "premium")) return;
    saving = true;
    const res = await quoteSubscription(branchId, {
      tier: selected,
      cycle,
      add_ons: { ...addOns, owner_login: 0 },
    });
    saving = false;
    if (res.ok) {
      quote = res.data;
      declined = false;
      payError = null;
      phase = "summary";
      await focus(() => summaryHeading);
      return;
    }
    await refuse(res);
  }

  async function refuse(res: ApiFailure) {
    switch (res.error.code) {
      case "BAND_NOT_AVAILABLE":
        formError = "pricing.band_unavailable";
        phase = "pick";
        await load();
        return;
      case "BILLING_DISABLED":
        formError = "onboarding.tier.error.billing_disabled";
        phase = "pick";
        await load();
        return;
      case "SUBSCRIPTION_EXISTS":
      case "FORBIDDEN":
        if (await leaveForCurrentStep(res.error.code)) return;
        formError = failure(res);
        return;
      default:
        formError = failure(res);
    }
  }

  const PLAN_NAMES: Record<Tier, MessageKey> = {
    free: "pricing.free.name",
    basic: "pricing.basic.name",
    premium: "pricing.premium.name",
  };

  async function pay() {
    const branch = wizard.state?.branch;
    if (paying || !quote || !branch) return;
    paying = true;
    declined = false;
    payError = null;
    const res = await createSubscription(branch.id, {
      tier: quote.tier,
      cycle: quote.cycle,
      add_ons: quote.add_ons,
      billing_country: branch.country,
    });
    if (!res.ok) {
      paying = false;
      if (res.error.code === "BAND_NOT_AVAILABLE") payError = "pricing.band_unavailable";
      else if (
        res.error.code === "SUBSCRIPTION_EXISTS" &&
        (await leaveForCurrentStep("SUBSCRIPTION_EXISTS"))
      ) {
        return;
      } else payError = failure(res);
      return;
    }
    let Razorpay;
    try {
      Razorpay = await loadRazorpay();
    } catch {
      paying = false;
      payError = "onboarding.pay.no_script";
      return;
    }
    // One outcome wins: a closed or failed Checkout can also report the other afterwards.
    let settled = false;
    const settle = (then: () => void) => {
      if (settled) return;
      settled = true;
      then();
    };
    try {
      const checkout = new Razorpay(
        checkoutOptions(res.data, t(PLAN_NAMES[res.data.tier]), {
          paid: () =>
            settle(() => {
              paying = false;
              wizard.confirming = true;
              phase = "confirming";
            }),
          dismissed: () => settle(() => (paying = false)),
        }),
      );
      checkout.on("payment.failed", () =>
        settle(() => {
          paying = false;
          declined = true;
          checkout.close();
        }),
      );
      checkout.open();
    } catch {
      settle(() => {
        paying = false;
        payError = "onboarding.pay.no_script";
      });
    }
  }

  function change() {
    phase = "pick";
    declined = false;
    payError = null;
  }

  // The 10.3 webhook marks this step done; the wizard (events, or the layout's 5 s poll) then
  // moves to step 3 by itself and this page follows once, announcing and focusing it.
  $effect(() => {
    if (phase !== "confirming" || leaving || upgrade) return;
    const state = wizard.state;
    if (state && state.current !== "tier") {
      leaving = true;
      void advance(state);
    }
  });

  // Upgrade mode has no wizard to watch: the plan itself is polled until the entitlement shows.
  $effect(() => {
    if (!upgrade || phase !== "confirming") return;
    const timer = setInterval(async () => {
      if (leaving) return;
      const res = await getTierChoices();
      const now = res.ok ? res.data.current : null;
      if (leaving || (now !== "basic" && now !== "premium")) return;
      leaving = true;
      upgrade.onDone();
    }, CONFIRM_POLL_MS);
    return () => clearInterval(timer);
  });

  $effect(() => {
    if (phase !== "confirming") return;
    slow = false;
    const timer = setTimeout(() => (slow = true), CONFIRM_SLOW_MS);
    return () => clearTimeout(timer);
  });

  onDestroy(() => {
    wizard.confirming = false;
  });

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (saving || !selected || !canContinue) return;
    formError = null;
    if (buying) {
      await review();
      return;
    }
    saving = true;
    const res = await chooseTier(selected, wizard.state?.branch?.id);
    saving = false;
    if (res.ok) {
      await advance(res.data);
      return;
    }
    switch (res.error.code) {
      case "ONE_FREE_BRANCH":
        formError = "onboarding.tier.error.one_free_branch";
        await load();
        return;
      case "BILLING_DISABLED":
        formError = "onboarding.tier.error.billing_disabled";
        await load();
        return;
      case "BAND_NOT_AVAILABLE":
        formError = "pricing.band_unavailable";
        await load();
        return;
      case "FORBIDDEN":
        if (await leaveForCurrentStep("FORBIDDEN")) return;
        formError = failure(res);
        return;
      case "ALREADY_SET":
        formError = "onboarding.tier.error.already_set";
        await load();
        return;
      default:
        formError = failure(res);
    }
  }
</script>

<h1 tabindex="-1">{upgrade ? t("onboarding.upgrade.title") : t(STEP_NAMES.tier)}</h1>
{#if phase === "pick"}
  <p class="intro">
    {upgrade
      ? t("onboarding.upgrade.intro", { branch: wizard.state?.branch?.name ?? "" })
      : t("onboarding.tier.intro")}
  </p>
{/if}

{#if loadError}
  <p class="form-error" role="alert">
    <span class="glyph" aria-hidden="true">⚠</span><span>{t(loadError)}</span>
  </p>
  <div class="actions">
    <Button label="onboarding.account_setup.retry" onclick={load} />
  </div>
{:else if !choices}
  <p role="status">{t("onboarding.setup.loading")}</p>
{:else if phase === "confirming"}
  <p class="confirming" role="status" data-testid="confirming">
    {slow ? t("onboarding.pay.slow") : t("onboarding.pay.confirming")}
  </p>
{:else if phase === "summary" && quote}
  <CheckoutSummary
    {quote}
    timezone={wizard.state?.branch?.timezone ?? ""}
    {paying}
    failed={declined}
    error={payError}
    onpay={pay}
    onchange={change}
    bind:heading={summaryHeading}
  />
  {#if upgrade}
    <div class="actions">
      <Button label="onboarding.upgrade.back" variant="secondary" onclick={upgrade.onBack} />
    </div>
  {/if}
{:else}
  <form class="form" novalidate onsubmit={submit}>
    <TabControl items={PERIODS} label="onboarding.tier.period" bind:selected={period} />
    <fieldset class="plans">
      <legend class="visually-hidden">{t("onboarding.tier.plans")}</legend>
      <div class="grid" class:two={upgrade !== null}>
        {#each plans as plan (plan.key)}
          {@const option = optionOf(plan.key)}
          {@const available = option?.available ?? false}
          {@const inputId = `${id}-plan-${plan.key}`}
          <div
            class="plan"
            class:selected={selected === plan.key}
            class:unavailable={!available}
            data-testid="plan-{plan.key}"
          >
            <div class="head">
              <span class="radio">
                <input
                  id={inputId}
                  type="radio"
                  name="tier"
                  value={plan.key}
                  checked={selected === plan.key}
                  disabled={!available}
                  aria-describedby={describedBy(inputId, Boolean(option?.reason))}
                  onchange={() => pickPlan(plan.key)}
                />
                <span class="dot" aria-hidden="true"></span>
              </span>
              <label class="name" for={inputId}>{t(plan.name)}</label>
            </div>
            <p class="price tabular-nums" id="{inputId}-price" data-testid="price-{plan.key}">
              {price(plan)}
            </p>
            {#if taxed(plan)}
              <p class="gst" data-testid="gst-{plan.key}">
                {t("pricing.gst", { percent: plan.gstPercent })}
              </p>
            {/if}
            {#if period === "yearly" && plan.key !== "free"}
              <p class="saving" data-testid="saving-{plan.key}">
                {t("onboarding.tier.yearly_saving")}
              </p>
            {/if}
            <ul class="features">
              {#each plan.features as feature (feature)}
                <li>{t(feature)}</li>
              {/each}
              {#if option}
                <li>{t("onboarding.tier.devices", { n: option.limits.devices })}</li>
                <li>
                  {option.limits.users >= UNLIMITED
                    ? t("onboarding.tier.users_unlimited")
                    : t("onboarding.tier.users", { n: option.limits.users })}
                </li>
              {/if}
            </ul>
            {#if choices.current === plan.key}
              <p class="current">{t("onboarding.tier.current")}</p>
            {/if}
            {#if option?.reason}
              <p class="reason" id="{inputId}-reason">{t(REASONS[option.reason])}</p>
            {/if}
          </div>
        {/each}
      </div>
    </fieldset>
    {#if buying && selected}
      <AddOnsPanel tier={selected} {band} {cycle} bind:addOns />
    {/if}
    {#if formError}
      <p class="form-error" role="alert">
        <span class="glyph" aria-hidden="true">⚠</span><span>{t(formError)}</span>
      </p>
    {/if}
    <div class="actions">
      <Button
        label="onboarding.tier.continue"
        type="submit"
        disabled={saving || !canContinue}
        disabledReason={saving ? "onboarding.tier.saving" : "onboarding.tier.choose_first"}
      />
      {#if upgrade}
        <Button label="onboarding.upgrade.back" variant="secondary" onclick={upgrade.onBack} />
      {/if}
    </div>
  </form>
{/if}

<style>
  .intro {
    margin: 0 0 var(--space-5);
    line-height: var(--type-hub-label-line-height);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .plans {
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }

  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-auto-rows: 1fr;
    gap: var(--space-3);
  }

  @media (min-width: 840px) {
    .grid {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }

    .grid.two {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  .plan {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    box-sizing: border-box;
    padding: var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background-color: var(--card-background);
  }

  .plan.selected {
    border: 2px solid var(--accent);
    padding: calc(var(--space-4) - 1px);
  }

  .plan.unavailable {
    border-style: dashed;
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .name {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
    cursor: pointer;
  }

  .unavailable .name {
    cursor: default;
  }

  .price {
    margin: 0;
    font-family: var(--type-web-display-family), system-ui, sans-serif;
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
    overflow-wrap: anywhere;
  }

  .gst,
  .saving,
  .current,
  .reason {
    margin: 0;
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
  }

  .gst {
    color: var(--text-muted);
  }

  .saving {
    font-weight: var(--type-hub-title-weight);
  }

  .confirming {
    margin: 0;
    font-size: var(--type-hub-title-size);
    line-height: var(--type-hub-label-line-height);
  }

  .current {
    font-weight: var(--type-hub-title-weight);
  }

  .reason {
    margin-top: auto;
    font-weight: var(--type-hub-title-weight);
  }

  .features {
    margin: 0;
    padding-left: var(--space-4);
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
  }

  .radio {
    position: relative;
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: var(--space-touch-min);
    height: var(--space-touch-min);
  }

  .radio input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }

  .radio input:disabled {
    cursor: default;
  }

  .dot {
    box-sizing: border-box;
    width: 1.5rem;
    height: 1.5rem;
    border: 2px solid var(--border-strong);
    border-radius: 50%;
    background: var(--text-field-background);
    pointer-events: none;
  }

  .radio:has(input:checked) .dot {
    border: 0.45rem solid var(--accent);
  }

  .radio:has(input:disabled) .dot {
    border-style: dashed;
  }

  .radio:has(input:focus-visible) .dot {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .form-error {
    display: flex;
    gap: var(--space-2);
    margin: 0;
    color: var(--text-field-error-text);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
