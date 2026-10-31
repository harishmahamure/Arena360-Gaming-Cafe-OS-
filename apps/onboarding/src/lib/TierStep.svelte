<script lang="ts">
  import { Button, PRICING, TabControl, currencyOf, formatMoney, priceFor, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { Tier, TierChoices, TierUnavailable } from "@arena/ui/generated";
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import { chooseTier, getTierChoices, getWizard, type ApiFailure } from "./api";
  import { STEP_NAMES, advance, setWizard, stepPath, wizard } from "./wizard.svelte";

  type Period = "monthly" | "yearly";
  type Plan = (typeof PRICING)[number];

  const id = $props.id();
  const PERIODS: { id: Period; label: MessageKey }[] = [
    { id: "monthly", label: "onboarding.tier.monthly" },
    { id: "yearly", label: "onboarding.tier.yearly" },
  ];
  const REASONS: Record<TierUnavailable, MessageKey> = {
    one_free_branch: "onboarding.tier.reason.one_free_branch",
    billing_disabled: "onboarding.tier.reason.billing_disabled",
  };

  let period: string = $state("monthly");
  let choices: TierChoices | null = $state(null);
  let loadError: MessageKey | null = $state(null);
  let selected: Tier | null = $state(null);
  let saving = $state(false);
  let formError: MessageKey | null = $state(null);

  const optionOf = (key: Tier) => choices?.tiers.find((o) => o.key === key) ?? null;
  const canContinue = $derived(selected !== null && optionOf(selected)?.available === true);

  function failure(f: ApiFailure): MessageKey {
    return f.offline ? "onboarding.error.offline" : "onboarding.error.generic";
  }

  async function leaveForCurrentStep(): Promise<boolean> {
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
      if (res.error.code === "FORBIDDEN" && (await leaveForCurrentStep())) return;
      loadError = failure(res);
      return;
    }
    choices = res.data;
    const current = res.data.current;
    const usable = (key: Tier | null | undefined) =>
      key ? res.data.tiers.some((o) => o.key === key && o.available) : false;
    if (usable(current)) selected = current ?? null;
    else if (!usable(selected)) selected = res.data.tiers.find((o) => o.available)?.key ?? null;
  }

  onMount(load);

  function describedBy(inputId: string, hasReason: boolean): string {
    const ids = [inputId + "-price"];
    if (hasReason) ids.push(inputId + "-reason");
    return ids.join(" ");
  }

  function price(plan: Plan): string {
    const symbol = currencyOf(plan.currency)?.symbol ?? plan.currency;
    const amount = formatMoney(priceFor(plan, period === "yearly" ? "yearly" : "monthly"), {
      exponent: 0,
      symbol,
      symbol_position: "before",
      number_format: "indian",
    });
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

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (saving || !selected || !canContinue) return;
    formError = null;
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
        if (await leaveForCurrentStep()) return;
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

<h1 tabindex="-1">{t(STEP_NAMES.tier)}</h1>
<p class="intro">{t("onboarding.tier.intro")}</p>

{#if loadError}
  <p class="form-error" role="alert">
    <span class="glyph" aria-hidden="true">⚠</span><span>{t(loadError)}</span>
  </p>
  <div class="actions">
    <Button label="onboarding.account_setup.retry" onclick={load} />
  </div>
{:else if !choices}
  <p role="status">{t("onboarding.setup.loading")}</p>
{:else}
  <form class="form" novalidate onsubmit={submit}>
    <TabControl items={PERIODS} label="onboarding.tier.period" bind:selected={period} />
    <fieldset class="plans">
      <legend class="visually-hidden">{t("onboarding.tier.plans")}</legend>
      <div class="grid">
        {#each PRICING as plan (plan.key)}
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
                  onchange={() => (selected = plan.key)}
                />
                <span class="dot" aria-hidden="true"></span>
              </span>
              <label class="name" for={inputId}>{t(plan.name)}</label>
            </div>
            <p class="price tabular-nums" id="{inputId}-price" data-testid="price-{plan.key}">
              {price(plan)}
            </p>
            {#if plan.gstPercent > 0}
              <p class="gst" data-testid="gst-{plan.key}">
                {t("pricing.gst", { percent: plan.gstPercent })}
              </p>
            {/if}
            <ul class="features">
              {#each plan.features as feature (feature)}
                <li>{t(feature)}</li>
              {/each}
              {#if option}
                <li>{t("onboarding.tier.devices", { n: option.limits.devices })}</li>
                <li>{t("onboarding.tier.users", { n: option.limits.users })}</li>
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
  .current,
  .reason {
    margin: 0;
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
  }

  .gst {
    color: var(--text-muted);
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
