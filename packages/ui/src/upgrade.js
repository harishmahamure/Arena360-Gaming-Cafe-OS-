import { getContext, setContext } from "svelte";

/** Where a `tier-lock` "Upgrade" link goes when its app has not said otherwise (the Hub). */
export const DEFAULT_UPGRADE_BASE = "https://start.arena360.cloud";

const KEY = Symbol.for("arena.upgrade");

/**
 * Tells every `TierLock` below this component where the upgrade picker lives (`base`, an origin or
 * "" for the same origin) and whether it may carry the current page as the way back.
 */
/**
 * @param {string} base
 * @param {boolean} returnHere
 */
export function provideUpgrade(base, returnHere) {
  setContext(KEY, { base, returnHere });
}

/** @returns {{ base: string, returnHere: boolean }} */
export function upgradeContext() {
  return getContext(KEY) ?? { base: DEFAULT_UPGRADE_BASE, returnHere: false };
}

/**
 * The picker link for a locked tier (Story 10.5): `tier` names what the lock needs, `branch` the
 * branch it is about (the picker asks when it is missing), `returnTo` the page to come back to.
 *
 * @param {string} base
 * @param {string} tier
 * @param {string | undefined} [branch]
 * @param {string | undefined} [returnTo]
 */
export function upgradeUrl(base, tier, branch, returnTo) {
  const q = new URLSearchParams({ tier: tier.toLowerCase() });
  if (branch) q.set("branch", branch);
  if (returnTo) q.set("return", returnTo);
  return base.replace(/\/$/, "") + "/upgrade?" + q.toString();
}
