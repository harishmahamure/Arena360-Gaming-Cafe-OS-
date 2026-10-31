// The Pricing page's tab and billing cycle live in the URL (`/pricing?tab=cafes&billing=yearly`) and
// on `<html>` (`data-pricing-tab`, `data-pricing-billing`), which the stylesheet turns into the
// visible panel and prices. The inline script in `pricing.astro` repeats `read` before first paint.

export const TABS = ["cafes", "partners"] as const;
export const CYCLES = ["monthly", "yearly"] as const;
export type PricingTab = (typeof TABS)[number];
export type Cycle = (typeof CYCLES)[number];

/** The URL's `name` parameter when it is one of `allowed`, else `fallback`. */
export function read<T extends string>(
  search: string,
  name: string,
  allowed: readonly T[],
  fallback: T,
): T {
  const value = new URLSearchParams(search).get(name);
  return allowed.find((a) => a === value) ?? fallback;
}

/** Records a choice in the URL (without a history entry) and on `<html>`. */
export function write(name: "tab" | "billing", value: string): void {
  const url = new URL(location.href);
  url.searchParams.set(name, value);
  history.replaceState(history.state, "", url);
  document.documentElement.setAttribute(`data-pricing-${name}`, value); // i18n-ignore
}
