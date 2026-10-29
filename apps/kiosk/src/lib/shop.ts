import type { ShopItem, ShopMenu } from "@arena/ui/generated";

// The Shop tab's rules (Story 6.30): what the service's `shop_menu` and `shop_image` answers
// hold, the filter rail and the search. The Hub decides what the menu says (whether orders are
// enabled, what is listed, what is available, what is popular); the kiosk only renders it.
// Pure, so the page, the tab and their tests share them.

/** How often the page asks for the menu while the Shop tab is open (a stock change shows in 2 s). */
export const SHOP_POLL_OPEN_MS = 1000;

/** How often it asks while another tab is open, to learn whether the Shop tab exists at all. */
export const SHOP_POLL_IDLE_MS = 30_000;

/** The Hub's `shop.menu_result`. */
export type ShopView = ShopMenu;

const isComponent = (v: unknown): boolean => {
  if (typeof v !== "object" || v === null) return false;
  const c = v as Record<string, unknown>;
  return typeof c.name === "string" && typeof c.qty === "number";
};

const isItem = (v: unknown): v is ShopItem => {
  if (typeof v !== "object" || v === null) return false;
  const i = v as Record<string, unknown>;
  return (
    typeof i.id === "string" &&
    typeof i.name === "string" &&
    typeof i.category === "string" &&
    typeof i.price_minor === "number" &&
    typeof i.bundle === "boolean" &&
    typeof i.available === "boolean" &&
    typeof i.has_image === "boolean" &&
    typeof i.popular === "boolean" &&
    Array.isArray(i.components) &&
    i.components.every(isComponent)
  );
};

/** The menu in a `shop_menu` answer body, or `null` when it is not one. */
export function shopMenuOf(body: unknown): ShopView | null {
  if (typeof body !== "object" || body === null) return null;
  const b = body as Record<string, unknown>;
  if (typeof b.enabled !== "boolean" || !Array.isArray(b.items) || !b.items.every(isItem)) {
    return null;
  }
  // A branch without the feature has no menu, whatever the answer says.
  return { enabled: b.enabled, items: b.enabled ? (b.items as ShopItem[]) : [] };
}

/**
 * The photo in a `shop_image` answer body for product `id`: a JPEG data URL, `null` when the
 * product has none, and `undefined` when the body is not an answer for that product.
 */
export function shopImageOf(body: unknown, id: string): string | null | undefined {
  if (typeof body !== "object" || body === null) return undefined;
  const b = body as Record<string, unknown>;
  if (b.id !== id) return undefined;
  if (b.image === null) return null;
  return typeof b.image === "string" && b.image.startsWith("data:image/jpeg;base64,")
    ? b.image
    : null;
}

/** Whether the Shop tab is shown: the Hub says orders are enabled on this branch. */
export const shopEnabled = (view: ShopView | null): boolean => view?.enabled === true;

/** Whether anything can be ordered now; if not, the Shop says the kitchen is closed. */
export const hasAvailable = (items: ShopItem[]): boolean => items.some((i) => i.available);

const fold = (s: string) => s.trim().toLowerCase();

/** The rail's items after "All": every category on the menu, once each, sorted. */
export function categoryLabels(items: ShopItem[]): string[] {
  const seen = new Map<string, string>();
  for (const i of items) {
    const key = fold(i.category);
    if (key !== "" && !seen.has(key)) seen.set(key, i.category.trim());
  }
  return [...seen.values()].sort((a, b) => fold(a).localeCompare(fold(b)));
}

/** The cards the active category (`null`: All) and the search text leave, in the Hub's order. */
export function visibleItems(
  items: ShopItem[],
  category: string | null,
  query: string,
): ShopItem[] {
  const needle = fold(query);
  return items.filter(
    (i) =>
      (category === null || fold(i.category) === fold(category)) &&
      (needle === "" || fold(i.name).includes(needle)),
  );
}
