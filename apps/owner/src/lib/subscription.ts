// Account -> Subscription (Story 10.8): what the cloud says of each branch's plan and the owner's
// invoices, and how those facts are worded. Every amount is minor units of the plan's own
// currency (the band's), formatted with `formatMoney`; the cloud prices both cycles from the
// price book locked onto the subscription, so nothing here computes a price.

import { formatMoney, t } from "@arena/ui";
import type {
  AccountSubscription,
  BranchPlan,
  InvoiceDownload,
  InvoiceList,
  InvoiceRow,
  PaymentMethodView,
  PlanPrice,
  PlanState,
} from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { load, probe, type Loaded } from "./api";
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
