// Prices on the plan cards and the add-ons panel come from the committed pricing snapshot for the
// branch's band (CI-checked against the live book). The summary never uses them: it shows the
// cloud's quote.

import { bandLines, currencyOf, formatMoney } from "@arena/ui";
import type { Band } from "@arena/ui/generated";

export type Cycle = "monthly" | "yearly";
export type PricedItem = "basic" | "premium" | "addon_devices_20" | "addon_tunnel";

/** Minor units as money in the currency: whole amounts without decimals (`₹200`), else with. */
export function money(minor: number, currency: string): string {
  const info = currencyOf(currency);
  const exponent = info?.exponent ?? 2;
  const unit = 10 ** exponent;
  const whole = minor % unit === 0;
  return formatMoney(whole ? minor / unit : minor, {
    exponent: whole ? 0 : exponent,
    symbol: info?.symbol ?? currency,
    symbol_position: "before",
    number_format: currency === "INR" ? "indian" : "international",
  });
}

/** The snapshot's amount for an item in a band and cycle, in minor units, with its currency. */
export function snapshotPrice(
  band: Band,
  item: PricedItem,
  cycle: Cycle,
): { minor: number; currency: string; gst: boolean } | null {
  const b = bandLines(band);
  const line = b.lines.find((l) => l.item === item && l.cycle === cycle);
  return line ? { minor: line.amount_minor, currency: b.currency, gst: line.gst_applicable } : null;
}
