import type { CreditView, PaymentMethod, PaymentRow } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";

export const METHODS: readonly PaymentMethod[] = ["cash", "upi", "card"];

export const creditOffered = (credit: CreditView | null | undefined): credit is CreditView =>
  credit != null && !credit.paused && credit.limit_minor > 0;

export const headroomOf = (credit: CreditView): number =>
  Math.max(0, credit.limit_minor - credit.outstanding_minor);

export function methodsFor(
  walletMinor: number | null | undefined,
  credit?: CreditView | null,
): readonly PaymentMethod[] {
  if (typeof walletMinor !== "number") return METHODS;
  return creditOffered(credit) ? [...METHODS, "wallet", "credit"] : [...METHODS, "wallet"];
}

export type Row = { method: PaymentMethod; amount: string; reference: string };

export type Tally = {
  paid: number;
  remaining: number;
  change: number;
  overpaid: boolean;
};

export type RowErrors = { amount?: MessageKey; reference?: MessageKey };

const CARD_REFERENCE_MAX = 64;

export function parseAmount(text: string, exponent: number): number | null {
  const match = /^(\d{1,9})(?:\.(\d*))?$/.exec(text.trim());
  if (!match) return null;
  const fraction = match[2] ?? "";
  if (fraction.length > exponent) return null;
  const minor = Number(match[1] + fraction.padEnd(exponent, "0"));
  return minor > 0 && Number.isSafeInteger(minor) ? minor : null;
}

export function amountText(minor: number, exponent: number): string {
  if (minor <= 0) return "";
  const digits = String(minor).padStart(exponent + 1, "0");
  const cut = digits.length - exponent;
  return exponent === 0 ? digits : [digits.slice(0, cut), digits.slice(cut)].join(".");
}

export function tally(total: number, rows: Row[], exponent: number): Tally {
  let paid = 0;
  let cash = 0;
  for (const row of rows) {
    const amount = parseAmount(row.amount, exponent) ?? 0;
    paid += amount;
    if (row.method === "cash") cash += amount;
  }
  const surplus = paid - total;
  if (surplus <= 0) return { paid, remaining: -surplus, change: 0, overpaid: false };
  if (surplus <= cash) return { paid, remaining: 0, change: surplus, overpaid: false };
  return { paid, remaining: 0, change: 0, overpaid: true };
}

export function rowErrors(row: Row, exponent: number, walletMinor = 0): RowErrors {
  const errors: RowErrors = {};
  const amount = parseAmount(row.amount, exponent);
  if (amount === null) errors.amount = "hub.settle.err.amount";
  else if (row.method === "wallet" && amount > walletMinor) errors.amount = "hub.settle.err.wallet";
  const reference = row.reference.trim();
  if (row.method === "upi" && !/^\d{4}$/.test(reference)) errors.reference = "hub.settle.err.upi";
  if (row.method === "card" && (reference === "" || [...reference].length > CARD_REFERENCE_MAX)) {
    errors.reference = "hub.settle.err.card";
  }
  return errors;
}

export const rowsValid = (rows: Row[], exponent: number, walletMinor = 0): boolean =>
  rows.every((row) => Object.keys(rowErrors(row, exponent, walletMinor)).length === 0);

export function toPayments(rows: Row[], change: number, exponent: number): PaymentRow[] {
  let giveBack = change;
  const payments: PaymentRow[] = [];
  for (const row of rows) {
    let amount = parseAmount(row.amount, exponent) ?? 0;
    if (row.method === "cash" && giveBack > 0) {
      const taken = Math.min(giveBack, amount);
      amount -= taken;
      giveBack -= taken;
    }
    if (amount === 0) continue;
    const reference = row.reference.trim();
    payments.push(
      row.method === "cash" || row.method === "wallet" || row.method === "credit"
        ? { method: row.method, amount_minor: amount }
        : { method: row.method, amount_minor: amount, reference },
    );
  }
  return payments;
}
