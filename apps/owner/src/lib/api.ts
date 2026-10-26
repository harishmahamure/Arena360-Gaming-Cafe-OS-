import { PUBLIC_API_BASE, PUBLIC_ONBOARDING_URL } from "$app/env/public";
import { clearCache, getCached, putCached } from "./cache";

export type { OwnerBranch } from "@arena/ui/generated";
export type Loaded<T> = { data: T; asOf: string; live: boolean };

const url = (path: string) => (PUBLIC_API_BASE ?? "").replace(/\/$/, "") + "/v1" + path;
export function setupUrl(branchId: string) {
  const u = new URL("/setup", PUBLIC_ONBOARDING_URL);
  u.searchParams.set("branch", branchId);
  return u.href;
}
export const signInUrl = () => PUBLIC_ONBOARDING_URL.replace(/\/$/, "") + "/sign-in";

async function fetchJson(path: string, method = "GET", body?: unknown): Promise<Response | null> {
  try {
    return await fetch(url(path), {
      method,
      credentials: "include",
      headers:
        body === undefined
          ? { accept: "application/json" }
          : { accept: "application/json", "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    return null;
  }
}

let epoch = 0;
let rotating: Promise<Response | null> | null = null;
const rotate = () =>
  (rotating ??= fetchJson("/session/refresh", "POST").finally(() => (rotating = null)));

export async function load<T>(path: string): Promise<Loaded<T> | "signed-out" | null> {
  const mine = epoch;
  let res = await fetchJson(path);
  if (res?.status === 401) {
    const fresh = await rotate();
    if (fresh?.ok) res = await fetchJson(path);
    else if (fresh && fresh.status < 500) return "signed-out";
  }
  if (res?.status === 401) return "signed-out";
  if (res?.ok) {
    try {
      const data = (await res.json()) as T;
      if (mine === epoch) await putCached(path, data);
      return { data, asOf: new Date().toISOString(), live: true };
    } catch {
      res = null;
    }
  }
  const hit = await getCached<T>(path);
  return hit ? { data: hit.payload, asOf: hit.as_of, live: false } : null;
}

export type Sent<T> = { ok: true; data: T } | { ok: false; code: string };

export async function send<T>(
  method: "POST" | "PATCH",
  path: string,
  body: unknown = {},
): Promise<Sent<T>> {
  let res = await fetchJson(path, method, body);
  if (res?.status === 401 && (await rotate())?.ok) res = await fetchJson(path, method, body);
  if (!res) return { ok: false, code: "OFFLINE" };
  const data = await res.json().catch(() => null);
  return res.ok ? { ok: true, data } : { ok: false, code: data?.code ?? "INTERNAL" };
}

export const dropCache = () => (epoch++, clearCache());

export async function signOut() {
  await fetchJson("/signout", "POST");
  await dropCache();
}
