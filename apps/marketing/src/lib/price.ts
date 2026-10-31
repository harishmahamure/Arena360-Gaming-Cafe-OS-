// Plan prices on Home come from the committed pricing snapshot (CI-checked against the live book),
// never typed by hand, so Home and the pricing page cannot drift apart.

import { currencyOf, formatMoney, planPrice } from "@arena/ui";

/** Whole rupees as money with Indian digit grouping: `₹1,999`. */
function rupees(whole: number): string {
  return formatMoney(whole, {
    exponent: 0,
    symbol: currencyOf("INR")?.symbol ?? "₹", // i18n-ignore
    symbol_position: "before",
    number_format: "indian",
  });
}

/** The monthly prices of Basic and Premium and the GST rate, as the text of "What you get". */
export function homePrices() {
  return {
    basic: rupees(planPrice("basic").monthly),
    premium: rupees(planPrice("premium").monthly),
    gst: String(planPrice("basic").gstPercent),
  };
}
