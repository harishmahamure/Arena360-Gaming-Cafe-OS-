<script lang="ts">
  import { Button, formatDateTime, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SubscriptionQuote } from "@arena/ui/generated";
  import { money } from "./tierPrice";

  let {
    quote,
    timezone,
    paying,
    failed,
    error,
    onpay,
    onchange,
    heading = $bindable(null),
  }: {
    quote: SubscriptionQuote;
    timezone: string;
    paying: boolean;
    /** Checkout reported a declined payment: nothing was charged. */
    failed: boolean;
    error: MessageKey | null;
    onpay: () => void;
    onchange: () => void;
    heading?: HTMLElement | null;
  } = $props();

  const planName = $derived(
    t(quote.tier === "premium" ? "pricing.premium.name" : "pricing.basic.name"),
  );
  const cycleName = $derived(
    t(quote.cycle === "yearly" ? "onboarding.tier.yearly" : "onboarding.tier.monthly"),
  );
  const addOnLines = $derived([
    ...(quote.add_ons.devices_20 > 0
      ? [t("onboarding.summary.devices", { n: quote.add_ons.devices_20 })]
      : []),
    ...(quote.add_ons.tunnel > 0 ? [t("onboarding.summary.tunnel")] : []),
  ]);
  const renews = $derived(formatDateTime(quote.renews_at, timezone, { dateStyle: "medium" }));
</script>

<section class="summary" data-testid="summary" aria-labelledby="summary-title">
  <h2 id="summary-title" tabindex="-1" bind:this={heading}>{t("onboarding.summary.title")}</h2>

  {#if failed}
    <p class="form-error" role="alert" data-testid="pay-failed">
      <span class="glyph" aria-hidden="true">⚠</span><span>{t("onboarding.pay.failed")}</span>
    </p>
  {/if}
  {#if error}
    <p class="form-error" role="alert">
      <span class="glyph" aria-hidden="true">⚠</span><span>{t(error)}</span>
    </p>
  {/if}

  <dl class="lines">
    <div>
      <dt>{t("onboarding.summary.plan")}</dt>
      <dd data-testid="summary-plan">{planName}</dd>
    </div>
    <div>
      <dt>{t("onboarding.summary.cycle")}</dt>
      <dd data-testid="summary-cycle">{cycleName}</dd>
    </div>
    <div>
      <dt>{t("onboarding.summary.add_ons")}</dt>
      <dd data-testid="summary-addons">
        {#if addOnLines.length === 0}
          {t("onboarding.summary.no_add_ons")}
        {:else}
          {addOnLines.join(", ")}
        {/if}
      </dd>
    </div>
    <div>
      <dt>{t("onboarding.summary.subtotal")}</dt>
      <dd class="tabular-nums" data-testid="summary-subtotal">
        {money(quote.amount_minor, quote.currency)}
      </dd>
    </div>
    {#if quote.gst_percent > 0}
      <div>
        <dt>{t("onboarding.summary.gst", { percent: quote.gst_percent })}</dt>
        <dd class="tabular-nums" data-testid="summary-gst">
          {money(quote.tax_minor, quote.currency)}
        </dd>
      </div>
    {/if}
    <div class="total">
      <dt>{t("onboarding.summary.total")}</dt>
      <dd class="tabular-nums" data-testid="summary-total">
        {money(quote.total_minor, quote.currency)}
      </dd>
    </div>
  </dl>

  <p class="note" data-testid="summary-renews">
    {t("onboarding.summary.renews", { date: renews })}
  </p>
  {#if quote.notice}
    <p class="note" data-testid="summary-notice">{quote.notice}</p>
  {/if}
  <p class="note" data-testid="summary-refund">{t("onboarding.summary.refund")}</p>

  <div class="actions">
    <Button
      label="onboarding.pay.button"
      onclick={onpay}
      disabled={paying}
      disabledReason="onboarding.pay.opening"
    />
    <Button label="onboarding.summary.change" variant="secondary" onclick={onchange} />
  </div>
</section>

<style>
  .summary {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  h2 {
    margin: 0;
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  h2:focus {
    outline: none;
  }

  .lines {
    display: flex;
    flex-direction: column;
    margin: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background-color: var(--card-background);
  }

  .lines > div {
    display: flex;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-4);
  }

  .lines > div + div {
    border-top: 1px solid var(--line);
  }

  dt {
    color: var(--text-muted);
  }

  dd {
    margin: 0;
    text-align: right;
    overflow-wrap: anywhere;
  }

  .total {
    font-weight: var(--type-hub-title-weight);
  }

  .total dt {
    color: inherit;
  }

  .note {
    margin: 0;
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
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
