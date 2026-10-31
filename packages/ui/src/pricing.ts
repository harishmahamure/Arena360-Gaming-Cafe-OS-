import type { MessageKey } from "./messages/index.js";
import { PRICING_SNAPSHOT } from "./pricing.snapshot.ts";

export type PlanKey = "free" | "basic" | "premium";
export type BillingPeriod = "monthly" | "yearly";
/** A purchasing-power band (Story 10.1): it fixes the currency a branch is billed in. */
export type PriceBand = keyof typeof PRICING_SNAPSHOT.bands;

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

/** Shown when a band is not on sale yet; Free stays selectable. */
export const BAND_UNAVAILABLE: MessageKey = "pricing.band_unavailable";

type SnapshotLine = {
  readonly item: string;
  readonly cycle: string;
  readonly amount_minor: number;
};

/** The live book's amount for an item in major units of the home band (INR), from the snapshot. */
function inr(item: "basic" | "premium", cycle: BillingPeriod): number {
  const lines: readonly SnapshotLine[] = PRICING_SNAPSHOT.bands.IN.lines;
  const line = lines.find((l) => l.item === item && l.cycle === cycle);
  if (!line) throw new Error(`pricing snapshot has no IN ${item} ${cycle} line`); // i18n-ignore
  return line.amount_minor / 100;
}

/**
 * The prices of one band from the generated snapshot of the live price book, or null while
 * the band is not on sale (the API answers `BAND_NOT_AVAILABLE` for it).
 */
export function bandPricing(band: PriceBand) {
  const b = PRICING_SNAPSHOT.bands[band];
  return b.enabled ? { band, currency: b.currency, lines: b.lines } : null;
}

/**
 * The snapshot's price lines of a band whether or not the snapshot has it on sale: the cloud
 * (`TierChoices`) decides what is on sale, the snapshot only holds the amounts.
 */
export function bandLines(band: PriceBand) {
  const b = PRICING_SNAPSHOT.bands[band];
  return {
    band,
    currency: b.currency,
    lines: b.lines as readonly (SnapshotLine & { gst_applicable: boolean })[],
  };
}

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
    monthly: inr("basic", "monthly"),
    yearly: inr("basic", "yearly"),
    gstPercent: GST_PERCENT,
    perBranch: true,
    features: ["pricing.basic.uploads", "pricing.basic.reports"],
  },
  premium: {
    key: "premium",
    name: "pricing.premium.name",
    currency: "INR",
    monthly: inr("premium", "monthly"),
    yearly: inr("premium", "yearly"),
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
