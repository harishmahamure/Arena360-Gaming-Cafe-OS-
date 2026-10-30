import type { OwnerBranch, SessionInfo } from "@arena/ui/generated";
import { dropCache, load, signInUrl } from "./api";

export const shell = $state({
  cafe: "",
  branches: null as OwnerBranch[] | null,
  asOf: "",
  offline: false,
  ready: false,
  /** The owner allowed text-message codes (offered beside the authenticator code in a step-up). */
  smsFallback: false,
});

export const isFree = () =>
  !!shell.branches?.length && shell.branches.every((b) => b.tier === "free");

/** A branch is live once Go live set `live_at` (Story 2.31). */
export const isLive = (b: OwnerBranch) => b.live_at !== null;

/**
 * The shared live-branch list: every branch-scoped screen takes its branches from here, so a
 * branch appears only once it is live and no screen carries its own filter.
 */
export const liveBranches = () => (shell.branches ?? []).filter(isLive);

/** Every branch still in setup: the screens say "Available after … goes live" for these. */
export const setupBranches = () => (shell.branches ?? []).filter((b) => !isLive(b));

let pending: Promise<void> | null = null;

export const refresh = () => (pending ??= run().finally(() => (pending = null)));

async function run() {
  const [session, branches] = await Promise.all([
    load<SessionInfo>("/session"),
    load<OwnerBranch[]>("/branches"),
  ]);
  if (session === "signed-out" || branches === "signed-out") {
    await dropCache();
    location.assign(signInUrl());
    return;
  }
  shell.cafe = session?.data.owner.cafe_name ?? shell.cafe;
  shell.smsFallback = session?.data.owner.sms_fallback ?? shell.smsFallback;
  shell.branches = branches?.data ?? shell.branches;
  shell.asOf = [session?.asOf, branches?.asOf].filter(Boolean).sort()[0] ?? "";
  shell.offline = !session?.live || !branches?.live;
  shell.ready = true;
}

export function age(asOf: string, now = Date.now()) {
  const m = Math.floor((now - Date.parse(asOf)) / 60000);
  if (m < 1) return ["owner.age.now", 0] as const;
  if (m < 60) return ["owner.age.minutes", m] as const;
  if (m < 1440) return ["owner.age.hours", Math.floor(m / 60)] as const;
  return ["owner.age.days", Math.floor(m / 1440)] as const;
}
