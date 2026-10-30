import type {
  AuditActor,
  AuditActorKind,
  AuditBuildStatus,
  AuditElevatedBy,
  AuditItem,
  AuditSearchPage,
} from "@arena/ui/generated";
import { t } from "@arena/ui";
import { messages, type MessageKey } from "@arena/ui/messages";
import { probe, send } from "./api";

/** How often a building month is asked about again. */
export const AUDIT_POLL_MS = 2_000;

/** The catalogue entries the screen offers; one search uses exactly one. */
export type AuditBy = "recent" | "actor" | "module" | "intent" | "player";
export type AuditQuery = {
  by: AuditBy;
  /** The actor kind (`by = actor`). */
  actorKind: string;
  /** An actor id (optional), a player id (`by = player`) or an intent kind (`by = intent`). */
  text: string;
  /** The intent domain (`by = module`). */
  module: string;
};

export const BY: readonly [AuditBy, MessageKey][] = [
  ["recent", "owner.audit.by.recent"],
  ["actor", "owner.audit.by.actor"],
  ["module", "owner.audit.by.module"],
  ["intent", "owner.audit.by.intent"],
  ["player", "owner.audit.by.player"],
];

/** The actors the owner can pick; a player is an `entity` search over the players table. */
export const ACTOR_KINDS: readonly [Exclude<AuditActorKind, "unknown">, MessageKey][] = [
  ["staff", "hub.audit.kind.staff"],
  ["owner", "hub.audit.kind.owner"],
  ["system", "hub.audit.kind.system"],
  ["device", "hub.audit.kind.device"],
  ["admin", "hub.audit.kind.admin"],
];

/** The intent domains offered as modules (the first part of an intent kind). */
export const MODULES: readonly [string, MessageKey][] = [
  ["session", "owner.audit.module.session"],
  ["wallet", "owner.audit.module.wallet"],
  ["credit", "owner.audit.module.credit"],
  ["refund", "owner.audit.module.refund"],
  ["cash", "owner.audit.module.cash"],
  ["order", "owner.audit.module.order"],
  ["stock", "owner.audit.module.stock"],
  ["product", "owner.audit.module.product"],
  ["rate", "owner.audit.module.rate"],
  ["plan", "owner.audit.module.plan"],
  ["coupon", "owner.audit.module.coupon"],
  ["staff", "owner.audit.module.staff"],
  ["settings", "owner.audit.module.settings"],
  ["device", "owner.audit.module.device"],
];

const WORD = /^[a-z0-9_.]{1,64}$/;

/** What is wrong with a search before it is sent, if anything. */
export function queryProblem(q: AuditQuery): MessageKey | null {
  const text = q.text.trim();
  if (q.by === "intent" && !WORD.test(text.toLowerCase())) return "owner.audit.problem.intent";
  if (q.by === "player" && !text) return "owner.audit.problem.player";
  return null;
}

/** The catalogue parameters of a search (without `month` and `cursor`). */
export function paramsOf(q: AuditQuery): Record<string, string> {
  const text = q.text.trim();
  switch (q.by) {
    case "actor":
      return { q: "actor", actor_kind: q.actorKind, ...(text ? { actor_id: text } : {}) };
    case "module":
      return { q: "module", module: q.module };
    case "intent":
      return { q: "intent", intent: text.toLowerCase() };
    case "player":
      return { q: "entity", tbl: "players", row_id: text };
    default:
      return { q: "recent" };
  }
}

export type AuditAnswer =
  | { state: "page"; page: AuditSearchPage }
  | { state: "building"; progress: number }
  | { state: "failed" }
  | { state: "missing" }
  | { state: "error"; message: MessageKey }
  | { state: "signed-out" };

const ERRORS: Record<string, MessageKey> = {
  RATE_LIMITED: "owner.audit.error.rate_limited",
  FEATURE_NOT_ENTITLED: "owner.audit.error.tier",
  VALIDATION_FAILED: "owner.audit.error.search",
  UNSUPPORTED: "owner.audit.error.search",
};

const base = (branchId: string) => `/branches/${encodeURIComponent(branchId)}/audit`;
const generic = { state: "error", message: "owner.audit.error.generic" } as const;

/** Asks for one page of a month; `cursor` continues the page before. */
export async function searchAudit(
  branchId: string,
  month: string,
  q: AuditQuery,
  cursor?: string,
): Promise<AuditAnswer> {
  const params = new URLSearchParams({ month, ...paramsOf(q), ...(cursor ? { cursor } : {}) });
  const res = await probe(`${base(branchId)}?${params}`); // i18n-ignore
  if (res === "signed-out") return { state: "signed-out" };
  if (!res) return generic;
  const body = res.data as { code?: string; progress?: number } | null;
  if (res.status === 200 && body) return { state: "page", page: res.data as AuditSearchPage };
  if (body?.code === "AUDIT_BUILDING")
    return { state: "building", progress: Math.max(0, Math.min(100, body.progress ?? 0)) };
  if (body?.code === "AUDIT_BUILD_FAILED") return { state: "failed" };
  if (body?.code === "NOT_FOUND") return { state: "missing" };
  return { state: "error", message: ERRORS[body?.code ?? ""] ?? generic.message };
}

/** Starts (or follows) the build of a month: the first search of it, or Retry after a failure. */
export async function buildAudit(branchId: string, month: string): Promise<AuditAnswer> {
  const res = await send<AuditBuildStatus>("POST", `${base(branchId)}/build`, { month }); // i18n-ignore
  if (!res.ok) {
    return { state: "error", message: ERRORS[res.code] ?? generic.message };
  }
  if (res.data.state === "failed") return { state: "failed" };
  return { state: "building", progress: res.data.state === "ready" ? 100 : res.data.progress };
}

const lookup = (key: string): MessageKey | null =>
  Object.hasOwn(messages, key) ? (key as MessageKey) : null;

/** An actor's shown name: a staff name or device label as sent, a fixed key translated. */
export function actorName(actor: AuditActor): string {
  const fixed = actor.kind === "system" || actor.kind === "owner" || actor.kind === "admin";
  const key = fixed && actor.name.startsWith("hub.audit.actor.") ? lookup(actor.name) : null;
  return key ? t(key) : actor.name;
}

/** An intent's label; a kind with none shows as it is (`settings.set`). */
export function intentName(kind: string): string {
  const key = lookup(`hub.audit.intent.${kind}`); // i18n-ignore
  return key ? t(key) : kind;
}

/** A module's label; a domain with none shows as it is. */
export function moduleName(module: string): string {
  const key = lookup(`owner.audit.module.${module}`) ?? lookup(`hub.audit.module.${module}`); // i18n-ignore
  return key ? t(key) : module;
}

/** "PIN by Priya" or "Owner MFA"; nothing for an elevation this build does not know. */
export function elevation(by: AuditElevatedBy | null): string {
  if (!by) return "";
  if (by.kind === "pin") return t("owner.audit.elevated.pin", { name: by.name });
  return by.kind === "mfa" ? t("owner.audit.elevated.mfa") : "";
}

export const opLabel = (op: AuditItem["op"]): MessageKey =>
  op === "insert"
    ? "hub.audit.op.insert"
    : op === "delete"
      ? "hub.audit.op.delete"
      : "hub.audit.op.update";
