import type { BusinessBuyer } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";

const ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
export const BUYER_NAME_MAX = 100;

/**
 * Whether `value` is a well-formed GSTIN (Story 5.20): 15 characters, a state code, the PAN shape,
 * entity number, `Z` and a valid check character. The Hub checks it again at settle
 * (`GSTIN_INVALID`); this only keeps Settle disabled while the field is wrong.
 */
export function gstinValid(value: string): boolean {
  if (!/^(0[1-9]|[1-2]\d|3[0-8]|97|99)[A-Z]{5}\d{4}[A-Z][1-9A-Z]Z[0-9A-Z]$/.test(value)) {
    return false;
  }
  let sum = 0;
  for (let i = 0; i < 14; i++) {
    const p = ALPHABET.indexOf(value[i] as string) * (1 + (i % 2));
    sum += Math.floor(p / 36) + (p % 36);
  }
  return ALPHABET[(36 - (sum % 36)) % 36] === value[14];
}

/** The text of the GSTIN field as the Hub stores it: trimmed and upper case. */
export const normaliseGstin = (text: string) => text.trim().toUpperCase();

export type BuyerErrors = { gstin?: MessageKey; legal_name?: MessageKey };

/** What is wrong with the two Business invoice fields; empty when the buyer can be sent. */
export function buyerErrors(gstinText: string, nameText: string): BuyerErrors {
  const errors: BuyerErrors = {};
  if (!gstinValid(normaliseGstin(gstinText))) errors.gstin = "hub.settle.business.err.gstin";
  const name = nameText.trim();
  if (name.length === 0 || name.length > BUYER_NAME_MAX) {
    errors.legal_name = "hub.settle.business.err.name";
  }
  return errors;
}

/** The buyer to send, or `undefined` when Business invoice is off. */
export function toBuyer(
  on: boolean,
  gstinText: string,
  nameText: string,
): BusinessBuyer | undefined {
  return on ? { gstin: normaliseGstin(gstinText), legal_name: nameText.trim() } : undefined;
}
