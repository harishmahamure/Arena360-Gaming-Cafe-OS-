import type { MessageKey } from "./messages/index.js";

export type PlanKey = "free" | "basic" | "premium";
export type BillingPeriod = "monthly" | "yearly";

export type PlanPrice = {
  key: PlanKey;
  name: MessageKey;
  currency: "INR";
  monthly: number;
  yearly: number;
  gstPercent: number;
  perBranch: boolean;
  features: readonly MessageKey[];
};

export const GST_PERCENT = 18;

const PLANS: Record<PlanKey, PlanPrice> = {
  free: {
    key: "free",
    name: "pricing.free.name",
    currency: "INR",
    monthly: 0,
    yearly: 0,
    gstPercent: 0,
    perBranch: false,
    features: ["pricing.free.branches", "pricing.free.ads"],
  },
  basic: {
    key: "basic",
    name: "pricing.basic.name",
    currency: "INR",
    monthly: 200,
    yearly: 2000,
    gstPercent: GST_PERCENT,
    perBranch: true,
    features: ["pricing.basic.uploads", "pricing.basic.reports"],
  },
  premium: {
    key: "premium",
    name: "pricing.premium.name",
    currency: "INR",
    monthly: 1999,
    yearly: 19990,
    gstPercent: GST_PERCENT,
    perBranch: false,
    features: [
      "pricing.premium.everything_basic",
      "pricing.premium.orders",
      "pricing.premium.standby",
    ],
  },
};

export const PRICING: readonly PlanPrice[] = [PLANS.free, PLANS.basic, PLANS.premium];

export function planPrice(key: PlanKey): PlanPrice {
  return PLANS[key];
}

export function priceFor(plan: PlanPrice, period: BillingPeriod): number {
  return period === "monthly" ? plan.monthly : plan.yearly;
}
