import type { MessageKey } from "@arena/ui/messages";

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

/**
 * A deposit or expense amount typed into the Shift screen, in minor units: more than zero, whole
 * numbers or up to `exponent` decimals. Anything else is `null`.
 */
export function parseAmount(text: string, exponent: number): number | null {
  const minor = parseFloat(text, exponent);
  return minor !== null && minor > 0 ? minor : null;
}

/** The categories an expense is filed under, in the order the form offers them. */
export const EXPENSE_CATEGORIES = [
  "supplies",
  "utilities",
  "repairs",
  "food_stock",
  "other",
] as const;

/** A receipt photo is stored as a JPEG of at most this many bytes. */
export const RECEIPT_MAX_BYTES = 200 * 1024;

/** The longest side the first attempt scales a receipt photo to, in pixels. */
const RECEIPT_FIRST_SIDE = 1600;

/**
 * Resizes a picture to a JPEG data URI of at most 200 KB: the longest side and the quality step
 * down until it fits. `null` when the file is not a picture or will not shrink enough.
 */
export async function resizeReceipt(file: Blob): Promise<string | null> {
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(file);
  } catch {
    return null;
  }
  try {
    for (let side = RECEIPT_FIRST_SIDE; side >= 400; side = Math.round(side * 0.75)) {
      const scale = Math.min(1, side / Math.max(bitmap.width, bitmap.height));
      const canvas = document.createElement("canvas");
      canvas.width = Math.max(1, Math.round(bitmap.width * scale));
      canvas.height = Math.max(1, Math.round(bitmap.height * scale));
      canvas.getContext("2d")?.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      for (const quality of [0.8, 0.65, 0.5, 0.35]) {
        const uri = canvas.toDataURL("image/jpeg", quality);
        const bytes = Math.floor(((uri.length - uri.indexOf(",") - 1) * 3) / 4);
        if (bytes <= RECEIPT_MAX_BYTES) return uri;
      }
    }
    return null;
  } finally {
    bitmap.close();
  }
}

/** The label of each expense category. */
export const EXPENSE_CATEGORY_LABELS: Record<(typeof EXPENSE_CATEGORIES)[number], MessageKey> = {
  supplies: "hub.shift.cash.category.supplies",
  utilities: "hub.shift.cash.category.utilities",
  repairs: "hub.shift.cash.category.repairs",
  food_stock: "hub.shift.cash.category.food_stock",
  other: "hub.shift.cash.category.other",
};
