/**
 * The float typed into the Shift screen, in minor units: whole numbers or up to `exponent`
 * decimals, zero allowed (a drawer can open empty). Anything else is `null`.
 */
export function parseFloat(text: string, exponent: number): number | null {
  const match = /^(\d{1,9})(?:\.(\d*))?$/.exec(text.trim());
  if (!match) return null;
  const fraction = match[2] ?? "";
  if (fraction.length > exponent) return null;
  const minor = Number(match[1] + fraction.padEnd(exponent, "0"));
  return Number.isSafeInteger(minor) ? minor : null;
}

/** A float in minor units as the field's text: `"0.00"` for none, `"50.00"` for 5,000 minor units. */
export function floatText(minor: number, exponent: number): string {
  const digits = String(Math.max(0, minor)).padStart(exponent + 1, "0");
  const cut = digits.length - exponent;
  return exponent === 0 ? digits : [digits.slice(0, cut), digits.slice(cut)].join(".");
}
