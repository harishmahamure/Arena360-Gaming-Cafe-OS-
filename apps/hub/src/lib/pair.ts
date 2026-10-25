import type { PairHubRequest } from "@arena/ui/generated";
import { errorOf } from "./auth";

export const PAIR_HUB_URL = "http://127.0.0.1:7409/ui/pair-hub";
export const PAIR_PATH = "/pair";
const TIMEOUT_MS = 20000;

export type PairOutcome =
  | { kind: "paired" }
  | { kind: "invalid" }
  | { kind: "not_entitled" }
  | { kind: "rate_limited"; retryAfterS: number }
  | { kind: "cloud_unreachable" }
  | { kind: "hub_unreachable" };

function seconds(...values: unknown[]): number {
  for (const v of values) {
    const n = typeof v === "string" ? Number(v.trim()) : v;
    if (typeof n === "number" && Number.isFinite(n) && n > 0) return Math.ceil(n);
  }
  return 60;
}

export async function pairHub(code: string): Promise<PairOutcome> {
  const body: PairHubRequest = { code };
  let res: Response;
  try {
    res = await fetch(PAIR_HUB_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
  } catch {
    return { kind: "hub_unreachable" };
  }
  if (res.ok) return { kind: "paired" };
  const err = await errorOf(res);
  switch (err?.code) {
    case "ALREADY_SET":
      return { kind: "paired" };
    case "PAIR_CODE_INVALID":
    case "PAIR_CODE_EXPIRED":
      return { kind: "invalid" };
    case "FEATURE_NOT_ENTITLED":
      return { kind: "not_entitled" };
    case "RATE_LIMITED":
      return {
        kind: "rate_limited",
        retryAfterS: seconds(err.retry_after_s, res.headers.get("retry-after")),
      };
    case "CLOUD_UNREACHABLE":
      return { kind: "cloud_unreachable" };
  }
  if (res.status === 401) return { kind: "invalid" };
  if (res.status === 429) {
    return { kind: "rate_limited", retryAfterS: seconds(res.headers.get("retry-after")) };
  }
  if (res.status === 503) return { kind: "cloud_unreachable" };
  return { kind: "hub_unreachable" };
}

export function formatCountdown(totalSeconds: number): string {
  const s = Math.max(0, Math.ceil(totalSeconds));
  return String(Math.floor(s / 60)) + ":" + String(s % 60).padStart(2, "0");
}

type TauriWindow = { close?: () => Promise<void> | void };
type TauriGlobal = { window?: { getCurrentWindow?: () => TauriWindow } };

export async function closeShellWindow(): Promise<boolean> {
  const tauri = (window as { __TAURI__?: TauriGlobal }).__TAURI__;
  const current = tauri?.window?.getCurrentWindow?.();
  if (typeof current?.close !== "function") return false;
  try {
    await current.close();
    return true;
  } catch {
    return false;
  }
}
