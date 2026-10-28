import type { ReasonCode, RefundInvoice, UiError } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { authHeaders, errorOf, getToken, noticeLocked, setToken } from "./auth";

export const REFUND_INVOICE_URL = "http://127.0.0.1:7409/ui/refund/invoice";

export type RefundLookup =
  | { kind: "ok"; invoice: RefundInvoice }
  | { kind: "not_found" }
  | { kind: "signed_out" }
  | { kind: "error"; error: UiError | null }
  | { kind: "unreachable" };

/** `GET /ui/refund/invoice?no=`: opens a settled invoice with its lines and credit notes. */
export async function loadRefundInvoice(no: string): Promise<RefundLookup> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  const url = new URL(REFUND_INVOICE_URL);
  url.searchParams.set("no", no.trim());
  let res: Response;
  try {
    res = await fetch(url.href, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
  } catch {
    return { kind: "unreachable" };
  }
  if (res.status === 401) {
    setToken(null);
    return { kind: "signed_out" };
  }
  if (res.ok) {
    try {
      return { kind: "ok", invoice: (await res.json()) as RefundInvoice };
    } catch {
      return { kind: "unreachable" };
    }
  }
  const error = await errorOf(res);
  noticeLocked(error);
  return error?.code === "INVOICE_NOT_FOUND" ? { kind: "not_found" } : { kind: "error", error };
}

/** The reasons a refund is made for, in the order the screen offers them. */
export const REFUND_REASONS: ReasonCode[] = ["changed_mind", "mistake", "hardware_issue", "other"];

export const REFUND_REASON_LABELS: Record<string, MessageKey> = {
  changed_mind: "hub.refund.reason.changed_mind",
  mistake: "hub.refund.reason.mistake",
  hardware_issue: "hub.refund.reason.hardware_issue",
  other: "hub.refund.reason.other",
};

export type RefundMethod = "cash" | "upi" | "card" | "wallet";

export const REFUND_METHODS: RefundMethod[] = ["cash", "upi", "card", "wallet"];

export const REFUND_METHOD_LABELS: Record<RefundMethod, MessageKey> = {
  cash: "hub.refund.method.cash",
  upi: "hub.refund.method.upi",
  card: "hub.refund.method.card",
  wallet: "hub.refund.method.wallet",
};

/** Why a plan line cannot be refunded. */
export const LINE_REFUSALS: Record<string, MessageKey> = {
  plan_used: "hub.refund.line.plan_used",
  plan_not_refundable: "hub.refund.line.plan_not_refundable",
};

/** The inline message for the Hub's refusal of `refund.create`. */
export function refundRefusal(code: string, field?: string | null): MessageKey {
  if (code === "INVOICE_NOT_FOUND") return "hub.refund.err.not_found";
  if (code === "REFUND_EXCEEDS_PAID") return "hub.refund.err.exceeds";
  if (code === "SHIFT_NOT_OPEN") return "hub.refund.err.shift";
  if (code === "REFERENCE_REQUIRED") return "hub.refund.err.reference";
  if (code === "PERMISSION_DENIED") return "hub.refusal.permission";
  if (code === "STALE") return "hub.refund.err.stale";
  if (code === "REASON_REQUIRED") return "hub.refund.err.reason";
  if (code === "VALIDATION_FAILED") {
    if (field === "method") return "hub.refund.err.method";
    if (field === "reference") return "hub.refund.err.reference";
    if (field === "note") return "hub.refund.err.note";
    if (field?.startsWith("lines")) return "hub.refund.err.lines";
    return "hub.refund.err.failed";
  }
  return "hub.refund.err.failed";
}
