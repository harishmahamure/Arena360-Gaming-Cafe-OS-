<script lang="ts">
  import { Button, formatDateTime, t } from "@arena/ui";
  import type { BranchPlan } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { shell } from "./shell.svelte";
  import { cancelPlan, money, requestRefund } from "./subscription";

  /**
   * Cancel plan (Story 10.14). The plan stays active to the end of the paid period, then the branch
   * moves to Free; there are no partial refunds. The policy and what changes on Free are shown
   * first; a second factor is asked for before the cancellation goes through. Inside 7 days of the
   * first payment (Story 10.15) the panel also offers "Cancel and get a full refund": the branch
   * moves to Free at once and an Arena 360 operator issues the refund. `ondone` runs once it is
   * stored, `onstale` when the plan changed under the owner.
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
  const end = $derived(
    plan.current_period_end
      ? formatDateTime(plan.current_period_end, timezone, { dateStyle: "medium" })
      : "",
  );
  const offer = $derived(plan.refund_offer);
  const refundAmount = $derived(offer ? money(offer.amount_minor, offer.currency) : "");
  const FREE_LINES = [
    "owner.cancel.free_users",
    "owner.cancel.free_devices",
    "owner.cancel.free_dashboards",
    "owner.cancel.free_backup",
    "owner.cancel.free_ads",
  ] as const satisfies readonly MessageKey[];

  let busy = $state(false);
  let notice: MessageKey | null = $state(null);

  async function confirm() {
    if (busy) return;
    busy = true;
    notice = null;
    const res = await cancelPlan(plan);
    busy = false;
    if (res === "declined") notice = "owner.cancel.declined";
    else if (res.ok) ondone(t("owner.cancel.done", { date: end }));
    else if (res.code === "STALE") onstale();
    else notice = "owner.cancel.failed";
  }

  async function refund() {
    if (busy) return;
    busy = true;
    notice = null;
    const res = await requestRefund(plan);
    busy = false;
    if (res === "declined") notice = "owner.cancel.declined";
    else if (res.ok) ondone(t("owner.refund.requested"));
    else if (res.code === "STALE") onstale();
    else if (res.code === "REFUND_NOT_ALLOWED") notice = "owner.refund.not_allowed";
    else notice = "owner.refund.failed";
  }
</script>

<form
  class="cancel"
  aria-labelledby="{id}-title"
  data-testid="cancel-plan-{plan.branch_id}"
  onsubmit={(e) => {
    e.preventDefault();
    void confirm();
  }}
>
  <h3 id="{id}-title" class="type-hub-title">
    {t("owner.cancel.title", { branch: plan.branch_name })}
  </h3>
  {#if offer}
    <section aria-labelledby="{id}-refund" data-testid="refund-offer">
      <h4 id="{id}-refund" class="type-hub-label">{t("owner.refund.title")}</h4>
      <p>{t("owner.refund.policy", { amount: refundAmount })}</p>
      <div class="actions">
        <Button
          label="owner.refund.confirm"
          labelParams={{ amount: refundAmount }}
          type="button"
          onclick={() => void refund()}
          disabled={busy}
          disabledReason="owner.refund.working"
        />
      </div>
    </section>
  {/if}
  <p data-testid="cancel-policy">{t("owner.cancel.policy", { date: end })}</p>
  <section aria-labelledby="{id}-free">
    <h4 id="{id}-free" class="type-hub-label">{t("owner.cancel.free_title")}</h4>
    <ul>
      {#each FREE_LINES as line (line)}
        <li>{t(line)}</li>
      {/each}
    </ul>
  </section>
  {#if notice}<p role="alert" class="error">{t(notice)}</p>{/if}
  <div class="actions">
    <Button
      label="owner.cancel.confirm"
      type="submit"
      variant={offer ? "secondary" : "primary"}
      disabled={busy}
      disabledReason="owner.cancel.working"
    />
    <Button label="owner.cancel.back" variant="secondary" onclick={oncancel} />
  </div>
</form>

<style>
  .cancel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  h3,
  h4,
  p {
    margin: 0;
  }
  ul {
    margin: var(--space-1) 0 0;
    padding-inline-start: var(--space-4);
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
