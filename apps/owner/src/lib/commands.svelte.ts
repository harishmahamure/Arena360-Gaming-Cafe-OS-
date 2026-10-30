import { formatDateTime, formatTime, t } from "@arena/ui";
import type {
  OwnerBranch,
  OwnerCommandList,
  OwnerCommandPosted,
  OwnerCommandPreview,
  OwnerCommandView,
  SummaryCatalogue,
} from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { load, send } from "./api";
import { age, refresh, shell } from "./shell.svelte";
import { fetchLive, live } from "./today.svelte";
import { ulid } from "./ulid";

/** A change the owner is about to send; `branch_id` is a branch id or `all`. */
export type CommandDraft = {
  kind: string;
  payload: Record<string, unknown>;
  branch_id: string;
  base_rev?: number;
  /** The idempotency key of this change: fixed once, so a second Send never queues it twice. */
  key?: string;
};

/** The window event the Manage screens use to open the preview (`detail` is a `CommandDraft`). */
export const PROPOSE_EVENT = "arena:propose-command";

/** How often a visible command list re-reads its rows (a row changes state within 10 s). */
export const POLL_MS = 10_000;
const PAGE = 50;

export const cmd = $state({
  rows: [] as OwnerCommandView[],
  loaded: false,
  failed: false,
  draft: null as CommandDraft | null,
  preview: null as OwnerCommandPreview | null,
  previewing: false,
  previewFailed: false,
  sending: false,
  sendError: null as MessageKey | null,
  /** The text of the polite live region: what changed in the last poll. */
  announcement: "",
  /** Retry/Resend in flight or failed, by command id. */
  again: {} as Record<number, "sending" | "failed">,
});

/** Waiting commands of each branch, for the offline branch card on Today. */
export const waiting = $state<Record<string, OwnerCommandView[]>>({});

const KINDS: Record<string, { label: MessageKey; catalogue?: keyof SummaryCatalogue }> = {
  "rate.upsert": { label: "owner.cmd.kind.rate" },
  "plan.upsert": { label: "owner.cmd.kind.plan", catalogue: "plans" },
  "rule.upsert": { label: "owner.cmd.kind.rule", catalogue: "pricing_rules" },
  "coupon.upsert": { label: "owner.cmd.kind.coupon", catalogue: "coupons" },
  "product.upsert": { label: "owner.cmd.kind.product", catalogue: "products" },
  "backup.now": { label: "owner.cmd.kind.backup" },
};

/** What a command changes: its kind and, when the payload names the thing, the name. */
export function whatOf(kind: string, payload: unknown): string {
  const label = t(KINDS[kind]?.label ?? "owner.cmd.kind.other");
  const name = (payload as { name?: unknown } | null)?.name;
  return typeof name === "string" && name ? t("owner.cmd.what", { kind: label, name }) : label;
}

export const branchOf = (id: string): OwnerBranch | undefined =>
  shell.branches?.find((b) => b.id === id);

export type RowView = {
  tone: "info" | "ok" | "danger" | "muted";
  text: string;
  /** Retry (a rejected row) or Resend (an expired one); none for the rest. */
  action: "retry" | "resend" | null;
};

const ago = (ms: number, now: number) => {
  const [key, n] = age(new Date(ms).toISOString(), now);
  return t(key, { n });
};

export const queuedText = (row: OwnerCommandView, now: number) =>
  t("owner.cmd.queued", { age: ago(row.queued_at, now) });

/** The badge of a row: the server state in the owner's words. */
export function stateOf(row: OwnerCommandView, now: number): RowView {
  switch (row.status) {
    case "applied":
      return {
        tone: "ok",
        text: t("owner.cmd.state.applied", {
          time: formatTime(row.done_at ?? row.queued_at, branchOf(row.branch_id)?.timezone),
        }),
        action: null,
      };
    case "rejected":
      return row.error?.code === "TOKEN_REVOKED"
        ? { tone: "danger", text: t("owner.cmd.state.revoked"), action: null }
        : { tone: "danger", text: t("owner.cmd.state.rejected"), action: "retry" };
    case "expired":
      return { tone: "muted", text: t("owner.cmd.state.expired"), action: "resend" };
    default:
      return {
        tone: "info",
        text: t("owner.cmd.state.waiting", { age: ago(row.queued_at, now) }),
        action: null,
      };
  }
}

const dayKey = (ms: number, tz: string | undefined) =>
  formatDateTime(ms, tz, { year: "numeric", month: "2-digit", day: "2-digit" });

/** Today / Earlier, by the day in each row's branch time zone. */
export function groupOf(row: OwnerCommandView, now: number): "today" | "earlier" {
  const tz = branchOf(row.branch_id)?.timezone;
  return dayKey(row.queued_at, tz) === dayKey(now, tz) ? "today" : "earlier";
}

export const isWaiting = (r: OwnerCommandView) => r.status === "pending" || r.status === "fetched";

function merge(rows: OwnerCommandView[], fresh: OwnerCommandView[]) {
  const byId = new Map(rows.map((r) => [r.id, r]));
  for (const r of fresh) byId.set(r.id, r);
  return [...byId.values()].sort((a, b) => b.id - a.id);
}

/** Re-reads the owner's commands and announces, once, every row whose state changed. */
export async function refreshCommands() {
  const res = await load<OwnerCommandList>(`/commands?limit=${PAGE}`);
  if (res === "signed-out") {
    await refresh();
    return;
  }
  if (!res) {
    cmd.failed = !cmd.loaded;
    return;
  }
  const before = new Map(cmd.rows.map((r) => [r.id, r.status]));
  const said: string[] = [];
  for (const r of res.data.commands) {
    const was = before.get(r.id);
    if (cmd.loaded && was !== undefined && was !== r.status && !isWaiting(r)) {
      said.push(
        t("owner.cmd.announce", {
          what: whatOf(r.kind, r.payload),
          branch: branchOf(r.branch_id)?.name ?? r.branch_id,
          state: stateOf(r, Date.now()).text,
        }),
      );
    }
  }
  cmd.rows = merge(cmd.rows, res.data.commands);
  cmd.loaded = true;
  cmd.failed = false;
  if (said.length) cmd.announcement = said.join(". ");
}

/** The commands a branch has not finished: the "Waiting for branch" list of its Today card. */
export async function fetchWaiting(branchId: string) {
  const res = await load<OwnerCommandList>(
    `/commands?branch=${encodeURIComponent(branchId)}&limit=${PAGE}`,
  );
  if (res === "signed-out" || !res) return;
  waiting[branchId] = res.data.commands.filter(isWaiting);
}

/** Opens the preview for a change. */
export async function propose(draft: CommandDraft) {
  const mine = { ...draft, key: draft.key ?? ulid() };
  cmd.draft = mine;
  cmd.preview = null;
  cmd.previewFailed = false;
  cmd.previewing = true;
  cmd.sendError = null;
  const res = await send<OwnerCommandPreview>("POST", "/commands/preview", {
    branch_id: mine.branch_id,
    kind: mine.kind,
  });
  if (cmd.draft?.key !== mine.key) return; // cancelled, or replaced by another change
  cmd.previewing = false;
  if (res.ok) cmd.preview = res.data;
  else cmd.previewFailed = true;
}

export function cancel() {
  cmd.draft = null;
  cmd.preview = null;
  cmd.previewing = false;
  cmd.previewFailed = false;
  cmd.sendError = null;
}

/** Sends the previewed change. The cloud skips the branches that can't take it. */
export async function confirm() {
  const d = cmd.draft;
  if (!d || cmd.sending) return;
  cmd.sending = true;
  cmd.sendError = null;
  const res = await send<OwnerCommandPosted>("POST", "/commands", {
    branch_id: d.branch_id,
    idempotency_key: d.key,
    kind: d.kind,
    payload: d.payload,
    ...(d.base_rev === undefined ? {} : { base_rev: d.base_rev }),
  });
  cmd.sending = false;
  if (!res.ok) {
    cmd.sendError =
      res.code === "RATE_LIMITED" ? "owner.cmd.send.limited" : "owner.cmd.send.failed";
    return;
  }
  cmd.rows = merge(cmd.rows, res.data.commands);
  cmd.loaded = true;
  cancel();
}

/**
 * The `rev` the branch holds now for the row a command changes, from its newest summary; the
 * rev the old command carried when the summary doesn't list the row.
 */
export function currentBaseRev(row: OwnerCommandView): number | undefined {
  const p = row.payload as { id?: unknown; base_rev?: unknown } | null;
  const list = KINDS[row.kind]?.catalogue;
  const catalogue = live[row.branch_id]?.data?.summary?.catalogue;
  if (list && catalogue && typeof p?.id === "string") {
    const found = (catalogue[list] as { id: string; rev: number }[]).find((r) => r.id === p.id);
    if (found) return found.rev;
  }
  return typeof p?.base_rev === "number" ? p.base_rev : undefined;
}

/**
 * Retry (rejected) or Resend (expired): a new command with a new key and the current `base_rev`,
 * to this branch only. The old row stays in the history.
 */
export async function again(row: OwnerCommandView) {
  if (cmd.again[row.id] === "sending") return;
  cmd.again[row.id] = "sending";
  const b = branchOf(row.branch_id);
  if (b) await fetchLive(b);
  const { base_rev: _stale, ...fields } = (row.payload ?? {}) as Record<string, unknown>;
  const rev = currentBaseRev(row);
  const res = await send<OwnerCommandPosted>("POST", "/commands", {
    branch_id: row.branch_id,
    idempotency_key: ulid(),
    kind: row.kind,
    payload: fields,
    ...(rev === undefined ? {} : { base_rev: rev }),
  });
  if (!res.ok) {
    cmd.again[row.id] = "failed";
    return;
  }
  delete cmd.again[row.id];
  cmd.rows = merge(cmd.rows, res.data.commands);
}
