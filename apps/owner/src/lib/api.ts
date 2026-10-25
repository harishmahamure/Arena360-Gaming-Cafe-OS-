import { PUBLIC_API_BASE, PUBLIC_ONBOARDING_URL } from "$app/env/public";
import { clearCache, getCached, putCached } from "./cache";

export type OwnerBranch = {
  id: string;
  name: string;
  tier: "free" | "basic" | "premium";
  flags: string[];
};
export type Loaded<T> = { data: T; asOf: string; live: boolean };

const url = (path: string) => (PUBLIC_API_BASE ?? "").replace(/\/$/, "") + "/v1" + path;
export const signInUrl = () => PUBLIC_ONBOARDING_URL.replace(/\/$/, "") + "/sign-in";

async function fetchJson(path: string, method = "GET"): Promise<Response | null> {
  try {
    return await fetch(url(path), {
      method,
      credentials: "include",
      headers: { accept: "application/json" },
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

export const dropCache = () => (epoch++, clearCache());

export async function signOut() {
  await fetchJson("/signout", "POST");
  await dropCache();
}
