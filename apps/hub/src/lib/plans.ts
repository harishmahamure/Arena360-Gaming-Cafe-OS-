import type { PlanView } from "@arena/ui/generated";
import { authHeaders, getToken, setToken } from "./auth";

export const PLANS_URL = "http://127.0.0.1:7409/ui/plans";

export type PlansOutcome =
  { kind: "ok"; plans: PlanView[] } | { kind: "signed_out" } | { kind: "unreachable" };

export async function loadPlans(): Promise<PlansOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(PLANS_URL, {
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
      ? { kind: "ok", plans: body as PlanView[] }
      : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

/** Minutes from local midnight as `HH:MM`, the value of a time input. */
export function clock(minutes: number): string {
  const h = Math.floor(minutes / 60);
  return `${String(h).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`; // i18n-ignore (a time of day)
}

/** The minutes from local midnight of an `HH:MM` time input, or `null`. */
export function parseClock(text: string): number | null {
  const m = /^([01]\d|2[0-3]):([0-5]\d)$/.exec(text.trim());
  return m ? Number(m[1]) * 60 + Number(m[2]) : null;
}

/** A whole number of at least 1 (digits only), or `null`. */
export function parseWhole(text: string, max: number): number | null {
  if (!/^\d{1,7}$/.test(text.trim())) return null;
  const n = Number(text.trim());
  return n >= 1 && n <= max ? n : null;
}

/** Hours and minutes of a pack, e.g. `5 h` or `1 h 30 min`. */
export function hoursText(minutes: number): { h: number; m: number } {
  return { h: Math.floor(minutes / 60), m: minutes % 60 };
}
