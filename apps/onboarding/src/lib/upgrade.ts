// The upgrade entry (Story 10.5): where an "Upgrade" link or "Add branch" lands, and where the
// owner is sent when it is over. Pure helpers, so they can be tested without a browser.

import type { SubscriptionState, Tier } from "@arena/ui/generated";

/** States in which a branch already has a subscription of its own: no second checkout. */
const HAS_SUBSCRIPTION: readonly SubscriptionState[] = [
  "pending_review",
  "active",
  "renewal_needs_approval",
  "grace",
  "lapsed",
  "cancelled",
];

export const hasSubscription = (state: SubscriptionState): boolean =>
  HAS_SUBSCRIPTION.includes(state);

/** The tier a lock named (`?tier=basic`), or none. Free is never bought. */
export function lockedTier(raw: string | null): Tier | null {
  return raw === "basic" || raw === "premium" ? raw : null;
}

const rootOf = (origin: string) => origin.replace(/\/$/, "") + "/";

/**
 * Where to go back to: the page the link came from when it is on the Owner app or on this app,
 * otherwise the Owner app's home. Nothing else is followed, so the link cannot be used to send an
 * owner to another site.
 */
export function returnTarget(raw: string | null, ownerUrl: string, ownOrigin: string): string {
  const home = rootOf(ownerUrl);
  if (!raw) return home;
  let url: URL;
  try {
    url = new URL(raw, ownOrigin);
  } catch {
    return home;
  }
  if (url.protocol !== "https:" && url.protocol !== "http:") return home;
  const allowed = [new URL(ownerUrl).origin, ownOrigin];
  return allowed.includes(url.origin) ? url.href : home;
}

/** Account -> Subscription in the Owner app, opened on one branch. */
export function subscriptionUrl(ownerUrl: string, branchId: string): string {
  return rootOf(ownerUrl) + "account/subscription?branch=" + encodeURIComponent(branchId);
}
