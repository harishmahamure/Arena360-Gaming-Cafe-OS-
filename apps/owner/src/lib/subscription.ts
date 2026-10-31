// Account -> Subscription (Story 10.8): what the cloud says of each branch's plan and the owner's
// invoices, and how those facts are worded. Every amount is minor units of the plan's own
// currency (the band's), formatted with `formatMoney`; the cloud prices both cycles from the
// price book locked onto the subscription, so nothing here computes a price.

import { formatMoney, t } from "@arena/ui";
import type {
  AccountSubscription,
  AddOns,
  BranchPlan,
  ChangeConfirmed,
  CycleSwitchPreview,
  InvoiceDownload,
  InvoiceList,
  InvoiceRow,
  PaymentMethodView,
  PendingPlanChange,
  PlanChangePreview,
  PlanPrice,
  PlanState,
  Tier,
} from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { load, probe, send, type Loaded, type Sent } from "./api";
import { requestStepUp } from "./commands.svelte";
import { moneyLocale } from "./today.svelte";

export type Cycle = "monthly" | "yearly";
export const CYCLES = [
  { id: "monthly", label: "owner.subscription.cycle.monthly" },
  { id: "yearly", label: "owner.subscription.cycle.yearly" },
] as const satisfies readonly { id: Cycle; label: MessageKey }[];

/** The Monthly | Yearly choice in a link; anything else is Monthly. */
export const cycleOf = (value: string | null): Cycle => (value === "yearly" ? "yearly" : "monthly");

export const money = (minor: number, currency: string) => formatMoney(minor, moneyLocale(currency));

const BADGE_TONE: Record<PlanState, "info" | "ok" | "danger" | "muted"> = {
  free: "muted",
  pending: "info",
  pending_review: "info",
  active: "info",
  needs_approval: "info",
  grace: "danger",
  cancels: "muted",
  lapsed: "danger",
};
export const toneOf = (state: PlanState) => BADGE_TONE[state];

const STATE_TEXT: Record<PlanState, MessageKey> = {
  free: "owner.subscription.state.free",
  pending: "owner.subscription.state.pending",
  pending_review: "owner.subscription.state.pending_review",
  active: "owner.subscription.state.active",
  needs_approval: "owner.subscription.state.needs_approval",
  grace: "owner.subscription.state.grace",
  cancels: "owner.subscription.state.cancels",
  lapsed: "owner.subscription.state.lapsed",
};

/** The state in words; a cancelled plan names the day it ends (`date` is already formatted). */
export const stateText = (state: PlanState, date: string): string => t(STATE_TEXT[state], { date });

/** The add-ons with a quantity, in words. */
export function addOnLines(plan: BranchPlan): string[] {
  const out: string[] = [];
  if (plan.add_ons.devices_20 > 0)
    out.push(t("owner.subscription.addon.devices_20", { qty: plan.add_ons.devices_20 }));
  if (plan.add_ons.tunnel > 0) out.push(t("owner.subscription.addon.tunnel"));
  if (plan.add_ons.owner_login > 0)
    out.push(t("owner.subscription.addon.owner_login", { qty: plan.add_ons.owner_login }));
  return out;
}

/** The price of one cycle, with GST beside it on band IN (never folded in). */
export function priceText(price: PlanPrice, plan: BranchPlan): string {
  const currency = plan.currency ?? "INR";
  const amount = price.tax_minor
    ? t("owner.subscription.price.gst", {
        amount: money(price.amount_minor, currency),
        gst: money(price.tax_minor, currency),
      })
    : money(price.amount_minor, currency);
  return t(
    price.cycle === "yearly"
      ? "owner.subscription.price.yearly"
      : "owner.subscription.price.monthly",
    { amount },
  );
}

/** What the branch pays each renewal: the price of the cycle it is billed on. */
export const renewalPrice = (plan: BranchPlan) =>
  plan.prices.find((p) => p.cycle === plan.cycle) ?? null;

/** The payment method in words: a card by its last four digits, a mandate by its bank. */
export function methodText(method: PaymentMethodView | undefined): string {
  if (!method) return t("owner.subscription.payment.none");
  if (method.method === "upi") return t("owner.subscription.payment.upi");
  if (method.method === "card")
    return method.detail
      ? t("owner.subscription.payment.card", { last4: method.detail })
      : t("owner.subscription.payment.card_plain");
  return method.detail
    ? t("owner.subscription.payment.bank", { bank: method.detail })
    : t("owner.subscription.payment.bank_plain");
}

/** Branches in the order shown: the one an Upgrade link named first, the rest as the cloud lists them. */
export const ordered = (branches: BranchPlan[], wanted: string | null) =>
  [...branches].sort((a, b) => Number(b.branch_id === wanted) - Number(a.branch_id === wanted));

export const loadPlans = () => load<AccountSubscription>("/account/subscription");

/** The invoice list for a branch and a financial year (`""` means all of them). */
export function loadInvoices(branch: string, fy: string) {
  const q = new URLSearchParams();
  if (branch) q.set("branch", branch);
  if (fy) q.set("fy", fy);
  const query = q.toString();
  return load<InvoiceList>("/account/invoices" + (query ? "?" + query : ""));
}

export type { Loaded };

/** A signed link to one invoice's PDF, or why there is none. Never cached: the link expires. */
export async function pdfLink(row: InvoiceRow): Promise<{ url: string } | "signed-out" | "failed"> {
  const res = await probe(`/account/invoices/${encodeURIComponent(row.id)}/pdf`);
  if (res === "signed-out") return "signed-out";
  const body = res?.data as InvoiceDownload | null | undefined;
  return res?.status === 200 && typeof body?.url === "string" ? { url: body.url } : "failed";
}

/** Whether "Change plan" is offered: on an active plan it opens, while a renewal is unsettled it
 * is shown disabled with its reason, and every other state has no plan to change. */
export function changeOffer(plan: BranchPlan): "open" | "blocked" | "none" {
  if (plan.pending_change) return "none";
  if (plan.state === "active") return "open";
  if (plan.state === "needs_approval" || plan.state === "grace" || plan.state === "pending_review")
    return "blocked";
  return "none";
}

/** Whether "Change payment method" is offered: on an active plan, and while a renewal waits for
 * approval or payment, where a new method can settle it (Story 10.11). */
export function methodOffer(plan: BranchPlan): boolean {
  if (plan.pending_change) return false;
  return plan.state === "active" || plan.state === "needs_approval" || plan.state === "grace";
}

/** The date an unpaid renewal is about: the renewal due for one that needs approval, the end of
 * grace for one in grace. */
export const unpaidDate = (plan: BranchPlan): number | undefined =>
  plan.state === "grace" ? plan.grace_until : plan.current_period_end;

/** The change the owner picked, as the cloud takes it. */
export type Picked = { tier: Tier; add_ons: AddOns };

const changePath = (branchId: string, step: string) =>
  `/branches/${encodeURIComponent(branchId)}/subscription/change${step}`;

/** What the change would cost and when it starts. Nothing is written. */
export const previewChange = (plan: BranchPlan, picked: Picked) =>
  send<PlanChangePreview>("POST", changePath(plan.branch_id, "/preview"), {
    ...picked,
    rev: plan.rev,
  });

/** Confirms the change the owner saw, against the `rev` they saw it at. */
export const confirmChange = (plan: BranchPlan, picked: Picked) =>
  send<ChangeConfirmed>("POST", changePath(plan.branch_id, ""), { ...picked, rev: plan.rev });

/** "Keep current plan": withdraws a scheduled or held change. */
export const keepPlan = (plan: BranchPlan) =>
  send<null>("POST", changePath(plan.branch_id, "/withdraw"), { rev: plan.rev });

/** The plan a pending change leads to, in words: "Premium · Monthly". */
export const planName = (tier: Tier, cycle: Cycle | undefined) =>
  t("owner.subscription.plan_name", {
    tier: t(TIER_TEXT[tier]),
    cycle: t(
      cycle === "yearly" ? "owner.subscription.cycle.yearly" : "owner.subscription.cycle.monthly",
    ),
  });

export const TIER_TEXT = {
  free: "owner.tier.free",
  basic: "owner.tier.basic",
  premium: "owner.tier.premium",
} as const satisfies Record<Tier, MessageKey>;

const cyclePath = (branchId: string, step: string) =>
  `/branches/${encodeURIComponent(branchId)}/subscription/cycle${step}`;

/** What switching to `cycle` would cost and when it starts. Nothing is written. */
export const previewCycle = (plan: BranchPlan, cycle: Cycle) =>
  send<CycleSwitchPreview>("POST", cyclePath(plan.branch_id, "/preview"), {
    cycle,
    rev: plan.rev,
  });

/** Confirms the switch the owner saw; the answer carries the Checkout that authorises it. */
export const confirmCycle = (plan: BranchPlan, cycle: Cycle) =>
  send<ChangeConfirmed>("POST", cyclePath(plan.branch_id, ""), { cycle, rev: plan.rev });

/** Starts replacing the payment method; the answer carries the Checkout for the new mandate. */
export const changeMethod = (plan: BranchPlan) =>
  send<ChangeConfirmed>(
    "POST",
    `/branches/${encodeURIComponent(plan.branch_id)}/subscription/payment-method`,
    { rev: plan.rev },
  );

/** The cycle a branch can switch to: the one it is not on. */
export const otherCycle = (plan: BranchPlan): Cycle =>
  plan.cycle === "yearly" ? "monthly" : "yearly";

/** Whether the owner can still take a pending change back ("Keep current plan"). A plan change
 * can until it is being paid; a cycle switch or a new payment method only until the owner has
 * authorised it. */
export const keepable = (pending: PendingPlanChange): boolean =>
  pending.subject === "plan"
    ? pending.kind === "period_end" || pending.needs_mandate
    : pending.needs_mandate;

const cancelPath = (branchId: string, step: string) =>
  `/branches/${encodeURIComponent(branchId)}/subscription/cancel${step}`;

/** Whether "Cancel plan" is offered: an active plan with no change waiting (Story 10.14). */
export const cancelOffer = (plan: BranchPlan): boolean =>
  plan.state === "active" && !plan.pending_change;

/** Whether "Keep my plan" is offered: a plan set to cancel, before its period ends. */
export const keepOffer = (plan: BranchPlan): boolean => plan.state === "cancels";

/** Whether "Resubscribe" is offered: the branch runs at Free limits after a lapse or a cancel. */
export const resubscribeOffer = (plan: BranchPlan): boolean => plan.state === "lapsed";

/**
 * Cancels at the end of the period. The cloud wants a second factor first (`MFA_REQUIRED`): the
 * code prompt opens and the same request goes again once it passes; `"declined"` when the owner
 * closes the prompt. The prompt is asked again if the proof lapsed while the owner typed.
 */
export async function cancelPlan(plan: BranchPlan): Promise<Sent<null> | "declined"> {
  let res = await send<null>("POST", cancelPath(plan.branch_id, ""), { rev: plan.rev });
  while (!res.ok && res.code === "MFA_REQUIRED") {
    if (!(await requestStepUp())) return "declined";
    res = await send<null>("POST", cancelPath(plan.branch_id, ""), { rev: plan.rev });
  }
  return res;
}

/** "Keep my plan": withdraws the cancellation; the renewal date stays. */
export const keepSubscription = (plan: BranchPlan) =>
  send<null>("POST", cancelPath(plan.branch_id, "/withdraw"), { rev: plan.rev });
