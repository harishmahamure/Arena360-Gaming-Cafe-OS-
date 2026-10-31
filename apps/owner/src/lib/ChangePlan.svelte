<script lang="ts">
  import { Button, formatDateTime, t } from "@arena/ui";
  import type {
    AddOns,
    BranchPlan,
    PlanChangePreview,
    SubscriptionCheckout,
    Tier,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { checkoutOptions, loadRazorpay } from "./razorpay";
  import { shell } from "./shell.svelte";
  import { TIER_TEXT, confirmChange, money, previewChange, type Cycle } from "./subscription";

  const MAX_PACKS = 10;

  /**
   * Change plan (Story 10.9): pick a tier and add-ons, read what it costs, confirm. An upgrade
   * shows the prorated charge for the rest of the period; a lower plan shows the day it starts.
   * `ondone` runs once the change is stored (or paid, for a new mandate) so the card reloads.
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
  // svelte-ignore state_referenced_locally
  let tier: Tier = $state(plan.tier);
  // svelte-ignore state_referenced_locally
  let packs = $state(plan.add_ons.devices_20);
  // svelte-ignore state_referenced_locally
  let tunnel = $state(plan.add_ons.tunnel);
  let preview: PlanChangePreview | null = $state(null);
  let problem: MessageKey | string | null = $state(null);
  let loading = $state(false);
  let busy = $state(false);
  let failure: MessageKey | null = $state(null);

  const android = $derived(plan.hub_platform === "android");
  // A held Tunnel stays removable on Android; only adding it needs the Windows Hub.
  const tunnelLocked = $derived(android && plan.add_ons.tunnel === 0);
  const picked = $derived<{ tier: Tier; add_ons: AddOns }>({
    tier,
    add_ons: { devices_20: packs, tunnel: tier === "premium" ? 0 : tunnel, owner_login: 0 },
  });
  const unchanged = $derived(
    tier === plan.tier &&
      picked.add_ons.devices_20 === plan.add_ons.devices_20 &&
      picked.add_ons.tunnel === plan.add_ons.tunnel,
  );
  const cycle: Cycle = $derived(plan.cycle === "yearly" ? "yearly" : "monthly");
  const gst = (amount: number, tax: number, p: PlanChangePreview) =>
    tax
      ? t("owner.subscription.change.charge_gst", {
          amount: money(amount, p.currency),
          gst: money(tax, p.currency),
        })
      : money(amount, p.currency);

  // Each pick asks the cloud what it comes to; only the newest answer is shown.
  let latest = 0;
  $effect(() => {
    const body = picked;
    if (unchanged) {
      preview = null;
      problem = null;
      loading = false;
      return;
    }
    const mine = ++latest;
    loading = true;
    void previewChange(plan, body).then((res) => {
      if (mine !== latest) return;
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
  });

  function setPacks(raw: string) {
    const n = Math.trunc(Number(raw));
    packs = Number.isFinite(n) ? Math.min(MAX_PACKS, Math.max(0, n)) : 0;
  }

  async function openCheckout(p: PlanChangePreview, checkout: SubscriptionCheckout) {
    try {
      const Razorpay = await loadRazorpay();
      const box = new Razorpay(
        checkoutOptions(checkout, t(TIER_TEXT[p.tier]), {
          paid: () => ondone(t("owner.subscription.change.sent")),
          dismissed: () => {
            busy = false;
            failure = "owner.subscription.change.dismissed";
          },
        }),
      );
      box.on("payment.failed", () => {
        busy = false;
        failure = "owner.subscription.change.failed";
      });
      box.open();
    } catch {
      busy = false;
      failure = "owner.subscription.change.no_script";
    }
  }

  async function confirm() {
    if (!preview || busy) return;
    const shown = preview;
    busy = true;
    failure = null;
    const res = await confirmChange(plan, picked);
    if (!res.ok) {
      busy = false;
      if (res.code === "STALE") {
        onstale();
        failure = "owner.subscription.change.stale";
      } else failure = "owner.subscription.change.failed";
      return;
    }
    if (res.data.checkout) {
      await openCheckout(shown, res.data.checkout);
      return;
    }
    ondone(
      res.data.kind === "now"
        ? t("owner.subscription.change.sent")
        : t("owner.subscription.change.sent_scheduled", { date: day(res.data.effective_at) }),
    );
  }
</script>

<form
  class="change"
  aria-labelledby="{id}-title"
  data-testid="change-plan-{plan.branch_id}"
  onsubmit={(e) => {
    e.preventDefault();
    void confirm();
  }}
>
  <h3 id="{id}-title" class="type-hub-title">
    {t("owner.subscription.change.title", { branch: plan.branch_name })}
  </h3>

  <fieldset>
    <legend>{t("owner.subscription.change.tier")}</legend>
    {#each ["basic", "premium"] as const as key (key)}
      <label class="choice">
        <input type="radio" name="{id}-tier" value={key} bind:group={tier} />
        <span>{t(TIER_TEXT[key])}</span>
      </label>
    {/each}
  </fieldset>

  <fieldset>
    <legend>{t("owner.subscription.change.addons")}</legend>
    <div class="row">
      <label for="{id}-devices" class="name">{t("owner.subscription.change.devices")}</label>
      <input
        id="{id}-devices"
        class="qty tabular-nums"
        type="number"
        min="0"
        max={MAX_PACKS}
        step="1"
        inputmode="numeric"
        aria-describedby="{id}-devices-help"
        value={packs}
        onchange={(e) => {
          setPacks(e.currentTarget.value);
          e.currentTarget.value = String(packs);
        }}
      />
      <p class="help" id="{id}-devices-help">{t("owner.subscription.change.devices_help")}</p>
    </div>
    <div class="row">
      {#if tier === "premium"}
        <span class="name">{t("owner.subscription.change.tunnel")}</span>
        <p class="help" data-testid="tunnel-included">
          {t("owner.subscription.change.tunnel_included")}
        </p>
      {:else}
        <label class="check name">
          <input
            type="checkbox"
            checked={tunnel === 1}
            disabled={tunnelLocked}
            aria-describedby={tunnelLocked ? "{id}-tunnel-help" : undefined}
            onchange={(e) => (tunnel = e.currentTarget.checked ? 1 : 0)}
          />
          <span>{t("owner.subscription.change.tunnel")}</span>
        </label>
        {#if tunnelLocked}
          <p class="help" id="{id}-tunnel-help" data-testid="tunnel-windows">
            {t("owner.subscription.change.tunnel_windows")}
          </p>
        {/if}
      {/if}
    </div>
    <div class="row">
      <label class="check name">
        <input type="checkbox" disabled checked={false} aria-describedby="{id}-login-help" />
        <span>{t("owner.subscription.change.owner_login")}</span>
      </label>
      <p class="help" id="{id}-login-help">{t("owner.subscription.change.owner_login_soon")}</p>
    </div>
  </fieldset>

  <section class="summary" aria-labelledby="{id}-summary" aria-live="polite">
    <h4 id="{id}-summary" class="type-hub-label">{t("owner.subscription.change.summary")}</h4>
    {#if unchanged}
      <p>{t("owner.subscription.change.pick")}</p>
    {:else if loading}
      <p>{t("owner.subscription.change.loading")}</p>
    {:else if problem}
      <p class="error" role="alert">
        {problem.startsWith("owner.") ? t(problem as MessageKey) : problem}
      </p>
    {:else if preview}
      <dl class="facts">
        {#if preview.kind === "now"}
          <div>
            <dt>{t("owner.subscription.change.charge", { days: preview.days_left })}</dt>
            <dd class="tabular-nums" data-testid="change-charge">
              {gst(preview.charge_amount_minor, preview.charge_tax_minor, preview)}
            </dd>
          </div>
        {/if}
        <div>
          <dt>{t("owner.subscription.change.renewal")}</dt>
          <dd class="tabular-nums" data-testid="change-renewal">
            {t(
              cycle === "yearly"
                ? "owner.subscription.price.yearly"
                : "owner.subscription.price.monthly",
              {
                amount: gst(preview.renewal_amount_minor, preview.renewal_tax_minor, preview),
              },
            )}
          </dd>
        </div>
      </dl>
      {#if preview.kind === "period_end"}
        <p data-testid="change-later">
          {t("owner.subscription.change.scheduled", { date: day(preview.effective_at) })}
        </p>
      {:else}
        <p>{t("owner.subscription.change.now")}</p>
      {/if}
      {#if preview.needs_mandate}
        <p data-testid="change-mandate">{t("owner.subscription.change.mandate")}</p>
      {/if}
    {/if}
  </section>

  {#if failure}<p class="error" role="alert">{t(failure)}</p>{/if}
  <div class="actions">
    <Button
      label={preview?.needs_mandate
        ? "owner.subscription.change.confirm_mandate"
        : "owner.subscription.change.confirm"}
      type="submit"
      disabled={!preview || busy || loading}
      disabledReason="owner.subscription.change.pick"
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
  fieldset {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    margin: 0;
    padding: var(--space-3);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
  }
  legend {
    padding: 0 var(--space-1);
    font-weight: var(--type-hub-title-weight);
  }
  .choice,
  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
  }
  .choice input,
  .check input {
    width: 1.5rem;
    height: 1.5rem;
    margin: 0;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
  }
  .name {
    min-height: var(--space-touch-min);
    display: inline-flex;
    align-items: center;
    font-weight: var(--type-hub-title-weight);
  }
  .qty {
    box-sizing: border-box;
    width: 5rem;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--text-field-background);
    color: inherit;
    font: inherit;
  }
  .help {
    flex-basis: 100%;
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
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
