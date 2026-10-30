// Alerts → Alert limits (Story 8.34): the amounts above which a branch's Hub raises an alert. They
// are one setting, `owner_alert.thresholds` (a JSON `OwnerAlertThresholds`, in minor units), set at
// every branch by one `settings.set`. The Hub checks the same ranges.

import type { OwnerAlertThresholds } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import type { CommandDraft } from "./commands.svelte";
import { ALL, parsePrice, priceText } from "./pricing";

export const LIMITS_KEY = "owner_alert.thresholds";

/** What a branch uses until the owner sets limits (the Hub's defaults, in minor units). */
export const DEFAULT_LIMITS: OwnerAlertThresholds = {
  cash_variance_minor: null,
  large_refund_minor: 50_000,
  large_discount_minor: 20_000,
  large_discount_percent: 20,
};

const MAX_MINOR = 100_000_000;

/** The fields as the owner types them: amounts in major units, the percent whole. */
export type LimitsForm = { cash: string; refund: string; discount: string; percent: string };

export const formOf = (l: OwnerAlertThresholds, exponent: number): LimitsForm => ({
  cash: l.cash_variance_minor === null ? "" : priceText(l.cash_variance_minor, exponent),
  refund: priceText(l.large_refund_minor, exponent),
  discount: priceText(l.large_discount_minor, exponent),
  percent: String(l.large_discount_percent),
});

export type LimitsErrors = Partial<Record<keyof LimitsForm, MessageKey>>;

export type Checked =
  { ok: true; value: OwnerAlertThresholds } | { ok: false; errors: LimitsErrors };

/** The amounts are `0` or more and within the Hub's range; the percent is whole, 1 to 100. */
export function validateLimits(f: LimitsForm, exponent: number): Checked {
  const errors: LimitsErrors = {};
  const amount = (text: string): number | null => {
    const minor = parsePrice(text, exponent);
    return minor !== null && minor <= MAX_MINOR ? minor : null;
  };
  const cash = f.cash.trim() === "" ? null : amount(f.cash);
  if (f.cash.trim() !== "" && cash === null) errors.cash = "owner.alerts.limits.invalid_amount";
  const refund = amount(f.refund);
  if (refund === null) errors.refund = "owner.alerts.limits.invalid_amount";
  const discount = amount(f.discount);
  if (discount === null) errors.discount = "owner.alerts.limits.invalid_amount";
  const percent = /^\d{1,3}$/.test(f.percent.trim()) ? Number(f.percent) : Number.NaN;
  if (!(percent >= 1 && percent <= 100)) errors.percent = "owner.alerts.limits.invalid_percent";
  if (refund === null || discount === null || Object.keys(errors).length) {
    return { ok: false, errors };
  }
  return {
    ok: true,
    value: {
      cash_variance_minor: cash,
      large_refund_minor: refund,
      large_discount_minor: discount,
      large_discount_percent: percent,
    },
  };
}

/** One `settings.set` for every branch; the owner's change needs no `base_rev` (the owner wins). */
export const proposeLimits = (value: OwnerAlertThresholds): CommandDraft => ({
  kind: "settings.set",
  branch_id: ALL,
  payload: { key: LIMITS_KEY, value: JSON.stringify(value) },
});
