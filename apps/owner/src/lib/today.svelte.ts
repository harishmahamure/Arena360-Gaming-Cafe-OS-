import type { OwnerBranch, OwnerLive } from "@arena/ui/generated";
import { currencyOf, type formatMoney } from "@arena/ui";
import { load, send } from "./api";
import { getCached } from "./cache";
import { refresh } from "./shell.svelte";

type MoneyLocale = Parameters<typeof formatMoney>[1];

/** A branch's `live` answer as the screen holds it. */
export type LiveState = {
  data: OwnerLive | null;
  /** Reached the cloud on the last try (false: a cached copy, or nothing). */
  fresh: boolean;
  /** The last try found neither the cloud nor a cached copy. */
  failed: boolean;
};

export const live = $state<Record<string, LiveState>>({});
export const busy = $state({ refreshing: false });

/** How often a visible Today re-fetches a branch: Basic 15 min, Premium 5 min. */
export const CADENCE_MS = { basic: 15 * 60_000, premium: 5 * 60_000 } as const;
/** The tick that checks whether a branch is due. */
export const TICK_MS = 30_000;
/** A figure older than this during opening hours turns to warning ink. */
export const LATE_MS = 30 * 60_000;

const path = (id: string) => `/branches/${encodeURIComponent(id)}/live`;
const fetchedAt: Record<string, number> = {};
const reported = new Set<string>();

/** The branches whose figures Today shows: the paid ones (setup mode arrives with Story 8.14). */
export const showable = (branches: OwnerBranch[] | null) =>
  (branches ?? []).filter((b) => b.tier !== "free");

export const cadenceOf = (b: OwnerBranch) =>
  b.tier === "premium" ? CADENCE_MS.premium : CADENCE_MS.basic;

/** Money in the branch currency (INR groups the Indian way). */
export function moneyLocale(currency: string): MoneyLocale {
  const c = currencyOf(currency);
  return {
    exponent: c?.exponent ?? 2,
    symbol: c?.symbol ?? currency,
    symbol_position: "before",
    number_format: currency === "INR" ? "indian" : "international",
  };
}

/** Puts a cached copy on screen at once, for branches not yet shown. */
export async function showCached(branches: OwnerBranch[]) {
  await Promise.all(
    branches
      .filter((b) => !live[b.id])
      .map(async (b) => {
        const hit = await getCached<OwnerLive>(path(b.id));
        if (hit && !live[b.id]) live[b.id] = { data: hit.payload, fresh: false, failed: false };
      }),
  );
}

/** Fetches one branch. `report` sends the data age once per open of Today. */
export async function fetchLive(b: OwnerBranch, report = false, now = Date.now()) {
  fetchedAt[b.id] = now;
  const res = await load<OwnerLive>(path(b.id));
  if (res === "signed-out") {
    await refresh();
    return;
  }
  if (!res) {
    live[b.id] = { data: live[b.id]?.data ?? null, fresh: false, failed: !live[b.id]?.data };
    return;
  }
  live[b.id] = { data: res.data, fresh: res.live, failed: false };
  const asOf = res.data.as_of;
  if (res.live && report && asOf !== null && !reported.has(b.id)) {
    reported.add(b.id);
    void send("POST", `/branches/${encodeURIComponent(b.id)}/data-age`, {
      age_s: Math.max(0, Math.round((Date.now() - asOf) / 1000)),
    });
  }
}

/** Fetches every branch that is due (all of them when `force`). */
export async function fetchDue(branches: OwnerBranch[], force: boolean, report = false) {
  const now = Date.now();
  const due = branches.filter(
    (b) => force || now - (fetchedAt[b.id] ?? 0) >= cadenceOf(b) - TICK_MS / 2,
  );
  await Promise.all(due.map((b) => fetchLive(b, report, now)));
}

/** Forgets the reports of this open, so the next open of Today sends them again. */
export const newOpen = () => reported.clear();

export type Attention = {
  branch: string;
  currency: string;
  kind: string;
  ref: string;
  params: Record<string, string>;
};

/** Every branch's attention items: the branch heard from most recently first, then the Hub's order. */
export function attentionOf(branches: OwnerBranch[]): Attention[] {
  return branches
    .map((b) => ({ b, d: live[b.id]?.data }))
    .filter(
      (x): x is { b: OwnerBranch; d: OwnerLive & { summary: NonNullable<OwnerLive["summary"]> } } =>
        !!x.d?.summary,
    )
    .sort((x, y) => (y.d.as_of ?? 0) - (x.d.as_of ?? 0))
    .flatMap(({ b, d }) =>
      d.summary.attention.map((a) => ({
        branch: b.name,
        currency: d.currency,
        kind: a.kind,
        ref: a.ref,
        params: a.text_params,
      })),
    );
}
