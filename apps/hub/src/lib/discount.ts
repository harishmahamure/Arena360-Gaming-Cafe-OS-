import type {
  DiscountPreview,
  DiscountPreviewRequest,
  DiscountReason,
  IntentKind,
  ManualDiscount,
  UiError,
} from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { authHeaders, errorOf, getToken, noticeLocked, setToken } from "./auth";
import type { CouponSource } from "./coupons";

export const DISCOUNT_PREVIEW_URL = "http://127.0.0.1:7409/ui/discount/preview";

/** The reasons staff pick from, in the order the Hub offers them (Story 5.22). */
export const DISCOUNT_REASONS: { value: DiscountReason; label: MessageKey }[] = [
  { value: "regular_customer", label: "hub.settle.discount.reason.regular_customer" },
  { value: "service_issue", label: "hub.settle.discount.reason.service_issue" },
  { value: "staff_meal", label: "hub.settle.discount.reason.staff_meal" },
  { value: "other", label: "hub.settle.discount.reason.other" },
];

/** A discount staff chose for the bill, with the manager's elevation ticket when one was needed. */
export type AppliedDiscount = { discount: ManualDiscount; ticket?: string };

/** How long an elevation ticket is trusted before the PIN is asked for again (the Hub keeps 60 s). */
export const TICKET_FRESH_MS = 45_000;

/** The intent a bill is settled by, which a PIN approval has to name. */
export function settleKind(source: CouponSource): IntentKind {
  if ("order_id" in source) return "session.settle";
  if ("lines" in source) return "pos.sale";
  return "plan.sell";
}

export type DiscountAnswer =
  | { kind: "ok"; preview: DiscountPreview }
  | { kind: "refused"; message: string }
  | { kind: "signed_out" }
  | { kind: "unreachable" };

/**
 * Asks the Hub what the discount (after `coupon`, when one is applied) would do to the bill
 * (`POST /ui/discount/preview`). A discount or coupon the bill cannot take is `refused` with the
 * Hub's reason; nothing is written either way.
 */
export async function previewDiscount(
  discount: ManualDiscount,
  coupon: string | undefined,
  source: CouponSource,
): Promise<DiscountAnswer> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  const body: DiscountPreviewRequest = { discount, coupon, ...source };
  let res: Response;
  try {
    res = await fetch(DISCOUNT_PREVIEW_URL, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(5000),
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
      return { kind: "ok", preview: (await res.json()) as DiscountPreview };
    } catch {
      return { kind: "unreachable" };
    }
  }
  const error: UiError | null = await errorOf(res);
  noticeLocked(error);
  return error?.code === "COUPON_INVALID" ||
    (error?.code === "VALIDATION_FAILED" && (error.field ?? "").startsWith("discount"))
    ? { kind: "refused", message: error.message }
    : { kind: "unreachable" };
}
