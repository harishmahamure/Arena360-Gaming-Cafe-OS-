// The player's cart (Story 6.31): what is in it, how a stepper change is checked against the
// Hub's live stock, and how the Hub's answer to Place order reads. Pure, so the page, the panel
// and their tests share it.
//
// The Hub decides: the kiosk only keeps lines the player added and asks the Hub whether a step is
// possible (it carries `max_qty` on every menu item) and whether the order may be placed. There is
// no override on the kiosk and nothing is kept on disk.

import type { ShopItem, ShopPayOptions } from "@arena/ui/generated";

/** The most of one product a line may hold (the same bound the Hub accepts). */
export const MAX_LINE_QTY = 99;

/** The most distinct products one order may hold (the same bound the Hub accepts). */
export const MAX_CART_LINES = 50;

/** One line of the cart: the item as the Hub last priced it and how many. */
export type CartLine = { item: ShopItem; qty: number; notes?: string[] };

/** A line's kitchen notes (Story 7.2) as it keeps them: no blanks or repeats, sorted. */
export const normNotes = (notes: readonly string[] = []): string[] =>
  [...new Set(notes.map((n) => n.trim()).filter(Boolean))].sort((a, b) => a.localeCompare(b));

/** Two lines of one product are one line only when their notes are the same. */
export const lineKey = (l: { item: { id: string }; notes?: readonly string[] }): string =>
  JSON.stringify([l.item.id, normNotes(l.notes)]);

/** Why a line cannot have the quantity it has or is asked for. */
export type StockProblem =
  { kind: "out_of_stock" } | { kind: "only"; left: number } | { kind: "limit" };

/** What a stepper change came to: the cart after it, and the reason it was refused (if it was). */
export type StepResult = { lines: CartLine[]; problem: StockProblem | null };

/** How a cart is paid (Story 6.32): the session bill, the wallet now, or the counter. */
export type PayChoice = "bill" | "wallet" | "counter";

/** The choices a PC offers, from the Hub's menu answer (an older Hub offers the bill only). */
export type PayOffer = { bill: boolean; walletMinor: number | null; counter: boolean };

export function payOffer(options: ShopPayOptions | undefined): PayOffer {
  if (options === undefined) return { bill: true, walletMinor: null, counter: false };
  return {
    bill: options.bill,
    walletMinor: options.wallet_minor ?? null,
    counter: options.counter,
  };
}

/** Whether Pay now from wallet cannot cover `total`: it is shown disabled with the reason. */
export const walletShort = (offer: PayOffer, total: number): boolean =>
  offer.walletMinor !== null && offer.walletMinor < total;

/**
 * The choice to show selected: the player's own pick while the Hub still offers it, else the
 * session bill, then the wallet when it covers the cart, then the counter.
 */
export function chosenPay(offer: PayOffer, picked: PayChoice | null, total: number): PayChoice {
  const open = (c: PayChoice) =>
    c === "bill"
      ? offer.bill
      : c === "wallet"
        ? offer.walletMinor !== null && !walletShort(offer, total)
        : offer.counter;
  if (picked !== null && open(picked)) return picked;
  if (offer.bill) return "bill";
  if (open("wallet")) return "wallet";
  return "counter";
}

/** What Place order came to. */
export type OrderOutcome =
  | { kind: "ok" }
  /** The Hub refused: its code, its own words and the short product when it names one. */
  | {
      kind: "refused";
      code: string;
      message: string;
      productId: string | null;
      balanceMinor: number | null;
    }
  /** The Hub did not answer: the cart stays and Place order says the counter is reconnecting. */
  | { kind: "unreachable" };

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;

/** Why `item`, as the Hub now lists it, cannot supply `qty` units; `null` when it can. */
export function stockProblem(item: ShopItem | undefined, qty: number): StockProblem | null {
  if (item === undefined || !item.available || item.max_qty === 0) return { kind: "out_of_stock" };
  if (item.max_qty != null && qty > item.max_qty) return { kind: "only", left: item.max_qty };
  if (qty > MAX_LINE_QTY) return { kind: "limit" };
  return null;
}

/**
 * The cart after `delta` (+1 or -1) on product `id`. A step up is checked against `fresh`, the
 * item as the Hub lists it right now (`undefined`: it is no longer on the menu); a refused step
 * leaves the cart as it was. A step down to zero removes the line.
 */
export function stepCart(
  lines: CartLine[],
  id: string,
  delta: 1 | -1,
  fresh: ShopItem | undefined,
  notes: readonly string[] = [],
): StepResult {
  const key = lineKey({ item: { id }, notes });
  const at = lines.findIndex((l) => lineKey(l) === key);
  const qty = at < 0 ? 0 : lines[at].qty;
  // Lines of one product with different notes draw on the same stock.
  const held = lines.filter((l) => l.item.id === id).reduce((n, l) => n + l.qty, 0);
  if (delta < 0) {
    if (at < 0) return { lines, problem: null };
    return {
      lines:
        qty <= 1
          ? lines.filter((_, i) => i !== at)
          : lines.map((l, i) => (i === at ? { ...l, qty: qty - 1 } : l)),
      problem: null,
    };
  }
  // The Hub takes at most MAX_CART_LINES distinct products in one order.
  if (at < 0 && lines.length >= MAX_CART_LINES) return { lines, problem: { kind: "limit" } };
  const problem =
    stockProblem(fresh, held + 1) ?? (qty + 1 > MAX_LINE_QTY ? { kind: "limit" as const } : null);
  if (problem !== null || fresh === undefined) return { lines, problem };
  const kept = normNotes(notes);
  const line: CartLine = kept.length
    ? { item: fresh, qty: qty + 1, notes: kept }
    : { item: fresh, qty: qty + 1 };
  return {
    lines: at < 0 ? [...lines, line] : lines.map((l, i) => (i === at ? line : l)),
    problem: null,
  };
}

/**
 * Gives the line `from` of product `id` new kitchen notes. A line that already has them takes
 * its units (up to the line cap); the stock drawn is unchanged either way.
 */
export function renote(
  lines: CartLine[],
  id: string,
  from: readonly string[],
  to: readonly string[],
): CartLine[] {
  const fromKey = lineKey({ item: { id }, notes: from });
  const toKey = lineKey({ item: { id }, notes: to });
  const line = lines.find((l) => lineKey(l) === fromKey);
  if (!line || fromKey === toKey) return lines;
  const twin = lines.find((l) => lineKey(l) === toKey);
  const qty = Math.min(line.qty + (twin?.qty ?? 0), MAX_LINE_QTY);
  const kept = normNotes(to);
  const next: CartLine = kept.length
    ? { item: line.item, qty, notes: kept }
    : { item: line.item, qty };
  const at = lines.indexOf(twin ?? line);
  return lines
    .map((l, i) => (i === at ? next : l))
    .filter((l) => l === next || (lineKey(l) !== fromKey && lineKey(l) !== toKey));
}

/** The units in the cart, for the Shop's cart button. */
export const cartCount = (lines: CartLine[]): number => lines.reduce((n, l) => n + l.qty, 0);

/** The cart's total in minor units at the prices the Hub last listed (taxes are inclusive). */
export const cartTotal = (lines: CartLine[]): number =>
  lines.reduce((sum, l) => sum + l.item.price_minor * l.qty, 0);

/** The body of `shop_place`: product ids, units and chosen kitchen notes (the Hub finds the session). */
export const placeLines = (
  lines: CartLine[],
): { product_id: string; qty: number; kitchen_notes?: string[] }[] =>
  lines.map((l) =>
    l.notes?.length
      ? { product_id: l.item.id, qty: l.qty, kitchen_notes: l.notes }
      : { product_id: l.item.id, qty: l.qty },
  );

/**
 * Brings each line up to the Hub's latest listing: the price and the stock. A line whose item has
 * left the menu keeps its last listing (its stock problem is then `out_of_stock`).
 */
export function refreshLines(lines: CartLine[], menu: ShopItem[]): CartLine[] {
  return lines.map((l) => {
    const live = menu.find((i) => i.id === l.item.id);
    return live
      ? { ...l, item: live }
      : { ...l, item: { ...l.item, available: false, max_qty: 0 } };
  });
}

/** The outcome in the service's answer to `shop_place` (`shop.place_result`). */
export function placeOf(answer: unknown): OrderOutcome {
  if (!isObject(answer) || typeof answer.ok !== "boolean") return { kind: "unreachable" };
  // A link error (`HUB_SLOW`) or any error answer means the Hub did not take the order.
  if (!answer.ok) return { kind: "unreachable" };
  const body = answer.body;
  if (!isObject(body)) return { kind: "unreachable" };
  if (body.outcome === "ok") return { kind: "ok" };
  if (body.outcome === "refused") {
    return {
      kind: "refused",
      code: typeof body.code === "string" ? body.code : "",
      message: typeof body.message === "string" ? body.message : "",
      productId: typeof body.product_id === "string" ? body.product_id : null,
      balanceMinor: typeof body.balance_minor === "number" ? body.balance_minor : null,
    };
  }
  return { kind: "unreachable" };
}
