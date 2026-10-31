// The plan the Hub runs under (Story 10.6): the tier and billing cycle of its licence, as System
// and the "Plan updated" notice name them. Both come from the snapshot, never from a setting.

import type { BillingCycle, HubSnapshot, Tier } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";

export const TIER_LABELS: Record<Tier, MessageKey> = {
  free: "hub.plan.tier.free",
  basic: "hub.plan.tier.basic",
  premium: "hub.plan.tier.premium",
};

export const CYCLE_LABELS: Record<BillingCycle, MessageKey> = {
  monthly: "hub.plan.cycle.monthly",
  yearly: "hub.plan.cycle.yearly",
};

/** The plan line's parts, or `null` while the Hub does not know its tier yet. */
export function planOf(
  snapshot: HubSnapshot | null,
): { tier: Tier; cycle: BillingCycle | null; exp: number | null } | null {
  if (!snapshot?.tier) return null;
  const paid = snapshot.tier !== "free";
  return {
    tier: snapshot.tier,
    cycle: paid ? (snapshot.plan_cycle ?? null) : null,
    exp: paid ? (snapshot.licence_exp ?? null) : null,
  };
}
