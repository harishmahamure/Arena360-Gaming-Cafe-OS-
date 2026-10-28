import { t } from "@arena/ui";
import type {
  CouponPreview,
  CouponPreviewRequest,
  CouponView,
  PosSaleLine,
  UiError,
} from "@arena/ui/generated";
import { authHeaders, errorOf, getToken, noticeLocked, setToken } from "./auth";

export const COUPONS_URL = "http://127.0.0.1:7409/ui/coupons";
export const COUPON_PREVIEW_URL = "http://127.0.0.1:7409/ui/coupons/preview";
export const CODE_MIN = 3;
export const CODE_MAX = 24;

export type CouponsOutcome =
  { kind: "ok"; coupons: CouponView[] } | { kind: "signed_out" } | { kind: "unreachable" };

export async function loadCoupons(): Promise<CouponsOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(COUPONS_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    const body: unknown = res.ok ? await res.json() : null;
    return Array.isArray(body)
      ? { kind: "ok", coupons: body as CouponView[] }
      : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

/** The bill a coupon is priced on: an open bill, a counter cart or a plan, exactly one. */
export type CouponSource = { order_id: string } | { lines: PosSaleLine[] } | { plan_id: string };

export type CouponAnswer =
  | { kind: "ok"; preview: CouponPreview }
  | { kind: "refused"; message: string }
  | { kind: "signed_out" }
  | { kind: "unreachable" };

/**
 * Asks the Hub what `code` would do to the bill (`POST /ui/coupons/preview`). A coupon the bill
 * cannot take is `refused` with the Hub's reason; nothing is written either way.
 */
export async function previewCoupon(code: string, source: CouponSource): Promise<CouponAnswer> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  const body: CouponPreviewRequest = { code, ...source };
  let res: Response;
  try {
    res = await fetch(COUPON_PREVIEW_URL, {
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
      return { kind: "ok", preview: (await res.json()) as CouponPreview };
    } catch {
      return { kind: "unreachable" };
    }
  }
  const error: UiError | null = await errorOf(res);
  noticeLocked(error);
  return error?.code === "COUPON_INVALID"
    ? { kind: "refused", message: error.message }
    : { kind: "unreachable" };
}

/** Basis points of a percent coupon (`10`, `12.5`) from the text of an input, or `null`. */
export function parseCouponPercent(text: string): number | null {
  const m = /^(\d{1,3})(?:\.(\d{1,2}))?$/.exec(text.trim());
  if (!m) return null;
  const bp = Number(m[1]) * 100 + Number((m[2] ?? "").padEnd(2, "0") || 0);
  return bp >= 1 && bp <= 10_000 ? bp : null;
}

/** Basis points as the text of an input, e.g. `10` or `12.5`. */
export function couponPercentInput(bp: number): string {
  const value = bp / 100;
  return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(2)));
}

/** A code as the Hub stores it: trimmed and upper case. */
export const normaliseCode = (text: string) => text.trim().toUpperCase();

/** Whether `code` is 3 to 24 characters of `A-Z 0-9 - _`. */
export const codeValid = (code: string) =>
  /^[A-Z0-9_-]+$/.test(code) && code.length >= CODE_MIN && code.length <= CODE_MAX;

/** How many uses a coupon has left, in words ("3 of 10 used", "12 used"). */
export const usageText = (c: Pick<CouponView, "used" | "usage_limit">) =>
  c.usage_limit == null
    ? t("hub.coupons.used", { used: c.used })
    : t("hub.coupons.used_of", { used: c.used, limit: c.usage_limit });
