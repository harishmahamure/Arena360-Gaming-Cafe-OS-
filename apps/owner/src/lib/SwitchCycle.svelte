<script lang="ts">
  import { Button, formatDateTime, t } from "@arena/ui";
  import type { BranchPlan, CycleSwitchPreview, SubscriptionCheckout } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { checkoutOptions, loadRazorpay } from "./razorpay";
  import { shell } from "./shell.svelte";
  import {
    TIER_TEXT,
    confirmCycle,
    keepPlan,
    money,
    otherCycle,
    previewCycle,
  } from "./subscription";

  /**
   * Switch billing cycle (Story 10.10). To yearly: the yearly price less the unused days of the
   * paid month, charged today, with the next renewal a year out. To monthly: the day the paid year
   * ends, with no partial refund. Either way Checkout opens to authorise the new cycle's mandate
   * before anything changes; closing it leaves the old cycle in force. `ondone` runs once the owner
   * has authorised (or the change is stored) so the card reloads.
   */
  let {
    plan,
    oncancel,
    ondone,
    onstale,
  }: {
    plan: BranchPlan;
    oncancel: () => void;
    ondone: (message: string) => void;
    onstale: () => void;
  } = $props();

  const id = $props.id();
  const timezone = $derived(shell.branches?.find((b) => b.id === plan.branch_id)?.timezone);
  const day = (ms: number) => formatDateTime(ms, timezone, { dateStyle: "medium" });
  const target = $derived(otherCycle(plan));
  let preview: CycleSwitchPreview | null = $state(null);
  let problem: MessageKey | string | null = $state(null);
  let loading = $state(true);
  let busy = $state(false);
  let failure: MessageKey | null = $state(null);

  const gst = (amount: number, tax: number, p: CycleSwitchPreview) =>
    tax
      ? t("owner.subscription.change.charge_gst", {
          amount: money(amount, p.currency),
          gst: money(tax, p.currency),
        })
      : money(amount, p.currency);

  $effect(() => {
    const cycle = target;
    let alive = true;
    loading = true;
    void previewCycle(plan, cycle).then((res) => {
      if (!alive) return;
      loading = false;
      if (res.ok) {
        preview = res.data;
        problem = null;
      } else {
        preview = null;
        if (res.code === "STALE") onstale();
        problem = res.message ?? "owner.subscription.change.failed";
      }
    });
    return () => {
      alive = false;
    };
  });

  // Closing Checkout (or a failed payment) takes the stored switch back, so the old cycle stays.
  // Storing the switch moved the subscription's `rev` on by one.
  async function abandon() {
    const res = await keepPlan({ ...plan, rev: plan.rev + 1 });
    // A withdrawal that failed leaves the switch held: reload the card to show it as it is.
    ondone(res.ok ? t("owner.subscription.cycle.dismissed") : "");
  }

  async function openCheckout(checkout: SubscriptionCheckout, shown: CycleSwitchPreview) {
    try {
      const Razorpay = await loadRazorpay();
      const box = new Razorpay(
        checkoutOptions(checkout, t(TIER_TEXT[plan.tier]), {
          paid: () =>
            ondone(
              shown.kind === "now"
                ? t("owner.subscription.cycle.sent_yearly")
                : t("owner.subscription.cycle.sent_monthly", { date: day(shown.effective_at) }),
            ),
          dismissed: () => void abandon(),
        }),
      );
      // A failed attempt leaves Checkout open for a retry; closing it takes the switch back.
      box.open();
    } catch {
      // Storing the switch is done: take it back so it does not block further changes.
      await keepPlan({ ...plan, rev: plan.rev + 1 });
      busy = false;
      failure = "owner.subscription.change.no_script";
    }
  }

  async function confirm() {
    if (!preview || busy) return;
    const shown = preview;
    busy = true;
    failure = null;
    const res = await confirmCycle(plan, target);
    if (!res.ok) {
      busy = false;
      if (res.code === "STALE") {
        onstale();
        failure = "owner.subscription.change.stale";
      } else failure = "owner.subscription.change.failed";
      return;
    }
    if (res.data.checkout) await openCheckout(res.data.checkout, shown);
  }
</script>

<form
  class="change"
  aria-labelledby="{id}-title"
  data-testid="switch-cycle-{plan.branch_id}"
  onsubmit={(e) => {
    e.preventDefault();
    void confirm();
  }}
>
  <h3 id="{id}-title" class="type-hub-title">
    {t("owner.subscription.cycle.title", { branch: plan.branch_name })}
  </h3>

  <section class="summary" aria-labelledby="{id}-summary" aria-live="polite">
    <h4 id="{id}-summary" class="type-hub-label">{t("owner.subscription.change.summary")}</h4>
    {#if loading}
      <p>{t("owner.subscription.change.loading")}</p>
    {:else if problem}
      <p class="error" role="alert">
        {problem.startsWith("owner.") ? t(problem as MessageKey) : problem}
      </p>
    {:else if preview}
      <dl class="facts">
        <div>
          <dt>
            {t(
              preview.cycle === "yearly"
                ? "owner.subscription.cycle.yearly_price"
                : "owner.subscription.cycle.monthly_price",
            )}
          </dt>
          <dd class="tabular-nums" data-testid="cycle-price">
            {gst(preview.cycle_amount_minor, preview.cycle_tax_minor, preview)}
          </dd>
        </div>
        {#if preview.kind === "now"}
          <div>
            <dt>{t("owner.subscription.cycle.credit", { days: preview.days_left })}</dt>
            <dd class="tabular-nums" data-testid="cycle-credit">
              −{money(preview.credit_total_minor, preview.currency)}
            </dd>
          </div>
          <div>
            <dt>{t("owner.subscription.cycle.net")}</dt>
            <dd class="tabular-nums" data-testid="cycle-net">
              {gst(preview.charge_amount_minor, preview.charge_tax_minor, preview)}
            </dd>
          </div>
        {/if}
        <div>
          <dt>{t("owner.subscription.cycle.renews")}</dt>
          <dd class="tabular-nums" data-testid="cycle-renews">{day(preview.renews_at)}</dd>
        </div>
      </dl>
      {#if preview.kind === "period_end"}
        <p data-testid="cycle-later">
          {t("owner.subscription.cycle.scheduled", { date: day(preview.effective_at) })}
        </p>
      {:else}
        <p>{t("owner.subscription.cycle.now")}</p>
      {/if}
      {#if preview.notice}<p data-testid="cycle-notice">{preview.notice}</p>{/if}
      <p data-testid="cycle-mandate">{t("owner.subscription.cycle.mandate")}</p>
    {/if}
  </section>

  {#if failure}<p class="error" role="alert">{t(failure)}</p>{/if}
  <div class="actions">
    <Button
      label="owner.subscription.cycle.confirm"
      type="submit"
      disabled={!preview || busy || loading}
      disabledReason="owner.subscription.change.working"
    />
    <Button label="owner.subscription.change.cancel" variant="secondary" onclick={oncancel} />
  </div>
</form>

<style>
  .change {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  h3,
  h4,
  p {
    margin: 0;
  }
  .summary {
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
  .error {
    color: var(--danger);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
</style>
