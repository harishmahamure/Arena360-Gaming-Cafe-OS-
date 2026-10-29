// The POS screen's rules (Story 5.9): which products can be sold, what the search and the
// category strip show, the barcode lookup, the keyboard-wedge burst detector and the cart's
// live availability check. Pure functions over the `/ui/products` list; the page owns the state.

import type { PosSaleLine, Product } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import type { Tax } from "./tax";

/** The category strip's two fixed entries; every other entry is a product category. */
export const FAVOURITES = "\u0000favourites";
export const ALL = "\u0000all";

export const MAX_LINE_QTY = 999;
/** A scanner types faster than a person: keys under this many ms apart belong to one burst. */
export const SCAN_GAP_MS = 50;
/** The shortest burst taken as a barcode. */
export const SCAN_MIN_CHARS = 3;

/** The most characters a free-text kitchen note holds (Story 7.2). */
export const MAX_FREE_NOTE = 60;

/** `notes` are a kitchen line's chosen kitchen notes, kept sorted so equal sets read equal. */
export type CartLine = { productId: string; qty: number; notes?: string[] };
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

/** Kitchen notes as a line keeps them: trimmed, without blanks or repeats, sorted. */
export const normNotes = (notes: readonly string[] = []): string[] =>
  [...new Set(notes.map((n) => n.trim()).filter(Boolean))].sort((a, b) => a.localeCompare(b));

/** Two lines of one product are one line only when their notes are the same. */
export const lineKey = (l: { productId: string; notes?: readonly string[] }): string =>
  JSON.stringify([l.productId, normNotes(l.notes)]);

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

/** The cart with this product's line (the one with these notes) moved to `qty` (zero removes it). */
export function withQty(
  cart: CartLine[],
  productId: string,
  qty: number,
  notes: readonly string[] = [],
): CartLine[] {
  const key = lineKey({ productId, notes });
  const rest = cart.filter((l) => lineKey(l) !== key);
  if (qty <= 0) return rest;
  const at = cart.findIndex((l) => lineKey(l) === key);
  const kept = normNotes(notes);
  const line: CartLine = kept.length ? { productId, qty, notes: kept } : { productId, qty };
  return at < 0 ? [...rest, line] : [...rest.slice(0, at), line, ...rest.slice(at)];
}

/**
 * Gives the line `from` new kitchen notes. When another line of the product already has them
 * the two merge and their quantities add (up to the line cap).
 */
export function renote(
  cart: CartLine[],
  productId: string,
  from: readonly string[],
  to: readonly string[],
): CartLine[] {
  const fromKey = lineKey({ productId, notes: from });
  const toKey = lineKey({ productId, notes: to });
  const line = cart.find((l) => lineKey(l) === fromKey);
  if (!line || fromKey === toKey) return cart;
  const twin = cart.find((l) => lineKey(l) === toKey);
  const qty = Math.min(line.qty + (twin?.qty ?? 0), MAX_LINE_QTY);
  const kept = normNotes(to);
  const next: CartLine = kept.length ? { productId, qty, notes: kept } : { productId, qty };
  const at = cart.indexOf(twin ?? line);
  return cart
    .map((l, i) => (i === at ? next : l))
    .filter((l) => l === next || (lineKey(l) !== fromKey && lineKey(l) !== toKey));
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
  notes: readonly string[] = [],
): { cart: CartLine[]; refusal: Refusal | null } {
  const key = lineKey({ productId: product.id, notes });
  const current = cart.find((l) => lineKey(l) === key)?.qty ?? 0;
  const next = Math.min(current + delta, MAX_LINE_QTY);
  const moved = withQty(cart, product.id, next, notes);
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

/** What the Hub assumes before the branch tax loads: tax inside the price, none charged. */
export const DEFAULT_TAX: Tax = {
  country: "IN",
  gst: false,
  gstin: "",
  inclusive: true,
  product_bp: 500,
  play_bp: 1800,
  sac: "",
  label: "Tax",
  rate_bp: 0,
};

/** One rate of a bill: what it was charged on and the tax computed once for it. */
export type RateTax = { rate_bp: number; taxable_minor: number; tax_minor: number };
export type Bill = {
  subtotal_minor: number;
  tax_minor: number;
  total_minor: number;
  breakup: RateTax[];
};

const roundDiv = (n: bigint, d: bigint): bigint => {
  const abs = n < 0n ? -n : n;
  const q = (2n * abs + d) / (2n * d);
  return n < 0n ? -q : q;
};

/**
 * The Hub's tax computation (`invoice::tax::compute`): lines are summed per rate, the tax is
 * computed once per rate and rounded half-up to the minor unit once. Inclusive tax is carved out
 * of the price; exclusive tax is added on top. Integers throughout (BigInt), so a large line
 * cannot lose a minor unit.
 */
export function computeBill(
  lines: { rate_bp: number; amount_minor: number }[],
  inclusive: boolean,
): Bill {
  const perRate = new Map<number, bigint>();
  for (const l of lines)
    perRate.set(l.rate_bp, (perRate.get(l.rate_bp) ?? 0n) + BigInt(l.amount_minor));
  let subtotal = 0n;
  let taxSum = 0n;
  const breakup: RateTax[] = [];
  for (const rate_bp of [...perRate.keys()].sort((a, b) => a - b)) {
    const amount = perRate.get(rate_bp) ?? 0n;
    const r = BigInt(rate_bp);
    const tax = inclusive ? roundDiv(amount * r, 10000n + r) : roundDiv(amount * r, 10000n);
    const taxable = inclusive ? amount - tax : amount;
    subtotal += taxable;
    taxSum += tax;
    breakup.push({ rate_bp, taxable_minor: Number(taxable), tax_minor: Number(tax) });
  }
  return {
    subtotal_minor: Number(subtotal),
    tax_minor: Number(taxSum),
    total_minor: Number(subtotal + taxSum),
    breakup,
  };
}

/** The rate a product line is taxed at: its own in GST mode, the branch's simple rate otherwise. */
export const lineRate = (product: Product, tax: Tax): number =>
  tax.country === "IN" && tax.gst ? product.tax_rate_bp : tax.rate_bp;

/** The cart as the server will price it: per-rate tax, subtotal and total. */
export function cartBill(
  cart: CartLine[],
  byId: Map<string, Product>,
  tax: Tax = DEFAULT_TAX,
): Bill {
  const priced = cart.flatMap((l) => {
    const p = byId.get(l.productId);
    return p ? [{ rate_bp: lineRate(p, tax), amount_minor: p.price_minor * l.qty }] : [];
  });
  return computeBill(priced, tax.inclusive);
}

export const cartTotal = (
  cart: CartLine[],
  byId: Map<string, Product>,
  tax: Tax = DEFAULT_TAX,
): number => cartBill(cart, byId, tax).total_minor;

/**
 * The cart as the lines of `pos.sale`. `withNotes` off (no kitchen on the plan) leaves notes out,
 * and lines of one product that differed only by their notes become one line.
 */
export const saleLines = (cart: CartLine[], withNotes = true): PosSaleLine[] => {
  if (withNotes) {
    return cart.map((l) =>
      l.notes?.length
        ? { product_id: l.productId, qty: l.qty, kitchen_notes: l.notes }
        : { product_id: l.productId, qty: l.qty },
    );
  }
  const byProduct = new Map<string, PosSaleLine>();
  for (const l of cart) {
    const seen = byProduct.get(l.productId);
    if (seen) seen.qty += l.qty;
    else byProduct.set(l.productId, { product_id: l.productId, qty: l.qty });
  }
  return [...byProduct.values()];
};

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
      const fast = now - this.#last < SCAN_GAP_MS;
      this.reset();
      return fast && code.length >= SCAN_MIN_CHARS ? code : null;
    }
    if (key.length !== 1) return null;
    this.#chars = now - this.#last < SCAN_GAP_MS ? this.#chars + key : key;
    this.#last = now;
    return null;
  }

  reset() {
    this.#chars = "";
    this.#last = Number.NEGATIVE_INFINITY;
  }
}
