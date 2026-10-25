import type { SessionInfo } from "@arena/ui/generated";
import { dropCache, load, signInUrl, type OwnerBranch } from "./api";

export const shell = $state({
  cafe: "",
  branches: null as OwnerBranch[] | null,
  asOf: "",
  offline: false,
  ready: false,
});

export const isFree = () =>
  !!shell.branches?.length && shell.branches.every((b) => b.tier === "free");

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
