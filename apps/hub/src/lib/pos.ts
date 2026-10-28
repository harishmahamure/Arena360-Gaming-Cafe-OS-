// The POS screen's rules (Story 5.9): which products can be sold, what the search and the
// category strip show, the barcode lookup, the keyboard-wedge burst detector and the cart's
// live availability check. Pure functions over the `/ui/products` list; the page owns the state.

import type { Product } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";

/** The category strip's two fixed entries; every other entry is a product category. */
export const FAVOURITES = "\u0000favourites";
export const ALL = "\u0000all";

export const MAX_LINE_QTY = 999;
/** A scanner types faster than a person: keys under this many ms apart belong to one burst. */
export const SCAN_GAP_MS = 50;
/** The shortest burst taken as a barcode. */
export const SCAN_MIN_CHARS = 3;

export type CartLine = { productId: string; qty: number };
/** Why a step up was refused: `left` of `name` on hand, or (`cap`) the most one line holds. */
export type Refusal = { name: string; left: number; cap?: boolean };

const TAGS: Record<string, MessageKey> = {
  inactive: "hub.pos.tag.inactive",
  out_of_stock: "hub.pos.tag.out_of_stock",
  expired: "hub.pos.tag.expired",
};

/** The tag on a tile that cannot be added, or null when it can. */
export function unavailableTag(product: Product): MessageKey | null {
  if (product.available) return null;
  return TAGS[product.reason ?? ""] ?? "hub.pos.tag.unavailable";
}

export const categoriesOf = (products: Product[]): string[] =>
  [...new Set(products.map((p) => p.category).filter(Boolean))].sort((a, b) => a.localeCompare(b));

/**
 * The tiles to show. A search looks through every product by name (case-insensitive), whatever
 * category is picked; without one the picked category (or Favourites) decides.
 */
export function visibleProducts(products: Product[], category: string, query: string): Product[] {
  const q = query.trim().toLowerCase();
  if (q) return products.filter((p) => p.name.toLowerCase().includes(q));
  if (category === ALL) return products;
  if (category === FAVOURITES) return products.filter((p) => p.favourite);
  return products.filter((p) => p.category === category);
}

/** The product with exactly this barcode (inactive and out-of-stock ones included). */
export function findByBarcode(products: Product[], code: string): Product | undefined {
  const wanted = code.trim();
  return wanted ? products.find((p) => p.barcode === wanted) : undefined;
}

/** How much of each tracked product the cart takes: plain lines directly, bundles by part. */
function demand(cart: CartLine[], byId: Map<string, Product>): Map<string, number> {
  const out = new Map<string, number>();
  const take = (id: string, n: number) => out.set(id, (out.get(id) ?? 0) + n);
  for (const line of cart) {
    const p = byId.get(line.productId);
    if (!p) continue;
    if (p.bundle) for (const c of p.components) take(c.product_id, c.qty * line.qty);
    else take(p.id, line.qty);
  }
  return out;
}

/** The products a line draws stock from: itself, or a bundle's parts. */
const footprint = (p: Product): string[] =>
  p.bundle ? p.components.map((c) => c.product_id) : [p.id];

/** The first tracked product the cart takes more of than is on hand, among those a line uses. */
function shortOf(cart: CartLine[], product: Product, byId: Map<string, Product>): Refusal | null {
  const taken = demand(cart, byId);
  for (const id of footprint(product)) {
    const stock = byId.get(id);
    if (stock?.track_stock && (taken.get(id) ?? 0) > Math.max(stock.quantity, 0)) {
      return { name: stock.name, left: Math.max(stock.quantity, 0) };
    }
  }
  return null;
}

/** The cart with this product's line moved to `qty` (zero removes it). */
export function withQty(cart: CartLine[], productId: string, qty: number): CartLine[] {
  const rest = cart.filter((l) => l.productId !== productId);
  if (qty <= 0) return rest;
  const at = cart.findIndex((l) => l.productId === productId);
  const line = { productId, qty };
  return at < 0 ? [...rest, line] : [...rest.slice(0, at), line, ...rest.slice(at)];
}

/**
 * Moves a line by `delta`. A step up past what is on hand (counting a bundle's parts, and the
 * parts other lines already take) or past the line cap is refused with the reason; a step down
 * is always allowed.
 */
export function step(
  cart: CartLine[],
  product: Product,
  delta: number,
  byId: Map<string, Product>,
): { cart: CartLine[]; refusal: Refusal | null } {
  const current = cart.find((l) => l.productId === product.id)?.qty ?? 0;
  const next = Math.min(current + delta, MAX_LINE_QTY);
  const moved = withQty(cart, product.id, next);
  if (delta > 0) {
    if (current + delta > MAX_LINE_QTY) {
      return { cart, refusal: { name: product.name, left: MAX_LINE_QTY, cap: true } };
    }
    const refusal = shortOf(moved, product, byId);
    if (refusal) return { cart, refusal };
  }
  return { cart: moved, refusal: null };
}

/** A line whose product has since gone short (stock changed under the cart), or null. */
export const lineShortage = (
  cart: CartLine[],
  product: Product,
  byId: Map<string, Product>,
): Refusal | null => shortOf(cart, product, byId);

export const cartCount = (cart: CartLine[]): number => cart.reduce((n, l) => n + l.qty, 0);

export function cartTotal(cart: CartLine[], byId: Map<string, Product>): number {
  return cart.reduce((sum, l) => sum + (byId.get(l.productId)?.price_minor ?? 0) * l.qty, 0);
}

/**
 * Tells a barcode scanner (a keyboard wedge) from a person: printable keys under
 * `SCAN_GAP_MS` apart build up a code, and an Enter that follows at the same pace ends it.
 */
export class ScanBurst {
  #chars = "";
  #last = Number.NEGATIVE_INFINITY;

  /** Takes one key press; returns the code when this Enter ends a burst of at least 3 keys. */
  key(key: string, now: number): string | null {
    if (key === "Enter") {
      const code = this.#chars;
      const fast = now - this.#last <= SCAN_GAP_MS;
      this.reset();
      return fast && code.length >= SCAN_MIN_CHARS ? code : null;
    }
    if (key.length !== 1) return null;
    this.#chars = now - this.#last <= SCAN_GAP_MS ? this.#chars + key : key;
    this.#last = now;
    return null;
  }

  reset() {
    this.#chars = "";
    this.#last = Number.NEGATIVE_INFINITY;
  }
}
