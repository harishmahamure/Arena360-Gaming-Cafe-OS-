import { formatDateTime, formatTime, t } from "@arena/ui";
import type {
  OwnerBranch,
  OwnerCommandList,
  OwnerCommandPosted,
  OwnerCommandPreview,
  OwnerCommandTarget,
  OwnerCommandView,
  StaffSetupCode,
  SummaryCatalogue,
} from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { load, send, type Sent } from "./api";
import { getCached, queueAll, queueDelete, queuePut } from "./cache";
import { age, isLive, refresh, shell } from "./shell.svelte";
import { fetchLive, live, livePath } from "./today.svelte";
import { ulid } from "./ulid";

/**
 * What one branch gets of a change that differs per branch (Story 8.23): ids are per branch, so
 * each carries its own payload, `base_rev` and idempotency key.
 */
export type Variant = {
  branch_id: string;
  payload: Record<string, unknown>;
  base_rev?: number;
  key: string;
  /** The item is missing here: the owner picks "Create here" or "Exclude" before Send. */
  ask?: boolean;
};

/** A branch that gets nothing of the change, with the reason in the owner's words. */
export type Excluded = { branch_id: string; reason: string };

/** A change the owner is about to send; `branch_id` is a branch id or `all`. */
export type CommandDraft = {
  kind: string;
  payload: Record<string, unknown>;
  branch_id: string;
  base_rev?: number;
  /** The idempotency key of this change: fixed once, so a second Send never queues it twice. */
  key?: string;
  /** One part per branch when the payload differs per branch; the preview post stays `branch_id`. */
  variants?: Variant[];
  excluded?: Excluded[];
  /** The owner's answer for each branch whose variant asks. */
  choices?: Record<string, "create" | "exclude">;
  /** Keys of the variants the cloud already took, so a second Send skips them. */
  done?: string[];
};

/** One post of a draft. */
export type Part = Pick<Variant, "branch_id" | "payload" | "base_rev" | "key">;

/**
 * A change made with no signal, kept in IndexedDB until it can be sent (AD-14): the client ULID
 * key makes a resend after a lost response return the same server row.
 */
export type Queued = {
  key: string;
  /** The café it was made for; another owner's queue on this device is never shown or sent. */
  cafe: string;
  branch_id: string;
  kind: string;
  payload: Record<string, unknown>;
  base_rev?: number;
  created_at: number;
  /**
   * `queued` waits to send; `held` was refused `MFA_REQUIRED` and waits for the owner to pass a
   * step-up (Story 8.25); `expired` passed its 7 days; `refused` was refused by the cloud.
   */
  state: "queued" | "held" | "expired" | "refused";
};

/** A command row: the cloud's, or one standing for a change still queued on this device. */
export type Row = OwnerCommandView & { local?: Queued };

/** The age after which a queued change is not sent (the cloud's own command TTL). */
export const COMMAND_TTL_MS = 7 * 86_400_000;

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
  /** Set when the preview was built with no signal: each branch's cached `as_of`, null if never seen. */
  known: null as Record<string, string | null> | null,
  /** Changes made with no signal, oldest first. */
  queued: [] as Queued[],
  sending: false,
  /** Counts the changes sent or queued, so a form can close once its change left. */
  sent: 0,
  sendError: null as MessageKey | null,
  /** The text of the polite live region: what changed in the last poll. */
  announcement: "",
  /** Retry/Resend in flight or failed, by `rowKey`. */
  again: {} as Record<string, "sending" | "failed">,
  /** The setup codes a post issued (Story 8.26): shown once, kept only here until dismissed. */
  codes: [] as SetupCode[],
});

/** A setup code for a new staff member, with who it is for and where. */
export type SetupCode = StaffSetupCode & { name: string };

/** Keeps the setup codes of an answer so the owner can read them once. */
function takeCodes(posted: OwnerCommandPosted) {
  const issued = posted.setup_codes ?? [];
  if (!issued.length) return;
  const nameOf = (c: StaffSetupCode) => {
    const row = posted.commands.find(
      (r) =>
        r.branch_id === c.branch_id &&
        (r.payload as { staff_id?: unknown })?.staff_id === c.staff_id,
    );
    const name = (row?.payload as { display_name?: unknown } | undefined)?.display_name;
    return typeof name === "string" ? name : "";
  };
  cmd.codes = [...cmd.codes, ...issued.map((c) => ({ ...c, name: nameOf(c) }))];
}

/** The owner has read the codes: they are gone from the app. */
export const dismissCodes = () => (cmd.codes = []);

/** Waiting commands of each branch, for the offline branch card on Today. */
export const waiting = $state<Record<string, OwnerCommandView[]>>({});

const KINDS: Record<string, { label: MessageKey; catalogue?: (keyof SummaryCatalogue)[] }> = {
  "rate.upsert": { label: "owner.cmd.kind.rate" },
  "plan.upsert": { label: "owner.cmd.kind.plan", catalogue: ["plans"] },
  "rule.upsert": { label: "owner.cmd.kind.rule", catalogue: ["pricing_rules"] },
  "coupon.upsert": { label: "owner.cmd.kind.coupon", catalogue: ["coupons"] },
  "product.upsert": { label: "owner.cmd.kind.product", catalogue: ["products", "bundles"] },
  "stock.threshold": { label: "owner.cmd.kind.threshold" },
  "staff.upsert": { label: "owner.cmd.kind.staff", catalogue: ["staff"] },
  "staff.deactivate": { label: "owner.cmd.kind.staff_off", catalogue: ["staff"] },
  "role.upsert": { label: "owner.cmd.kind.role", catalogue: ["roles"] },
  "group.upsert": { label: "owner.cmd.kind.group", catalogue: ["groups"] },
  "device.assign": { label: "owner.cmd.kind.assign" },
  "device.revoke": { label: "owner.cmd.kind.revoke" },
  "backup.now": { label: "owner.cmd.kind.backup" },
  "credit.remind": { label: "owner.cmd.kind.credit_remind" },
};

/** What a command changes: its kind and, when the payload names the thing, the name. */
export function whatOf(kind: string, payload: unknown): string {
  const label = t(KINDS[kind]?.label ?? "owner.cmd.kind.other");
  const named = payload as { name?: unknown; code?: unknown; display_name?: unknown } | null;
  const name = named?.name ?? named?.code ?? named?.display_name;
  return typeof name === "string" && name ? t("owner.cmd.what", { kind: label, name }) : label;
}

export const rowKey = (r: Row) => (r.local ? `q:${r.local.key}:${r.branch_id}` : String(r.id)); // i18n-ignore

export const branchOf = (id: string): OwnerBranch | undefined =>
  shell.branches?.find((b) => b.id === id);

export type RowView = {
  tone: "info" | "ok" | "danger" | "muted";
  text: string;
  /** Retry (a rejected row), Resend (an expired one) or Send (a held one); none for the rest. */
  action: "retry" | "resend" | "send" | null;
};

const ago = (ms: number, now: number) => {
  const [key, n] = age(new Date(ms).toISOString(), now);
  return t(key, { n });
};

export const queuedText = (row: OwnerCommandView, now: number) =>
  t("owner.cmd.queued", { age: ago(row.queued_at, now) });

/** The badge of a row: the server state in the owner's words. */
export function stateOf(row: Row, now: number): RowView {
  if (row.local?.state === "queued")
    return { tone: "muted", text: t("owner.cmd.state.queued"), action: null };
  if (row.local?.state === "held")
    return { tone: "muted", text: t("owner.cmd.state.queued"), action: "send" };
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

/** The rows standing for the changes queued on this device, newest first. */
export function localRows(): Row[] {
  return cmd.queued
    .flatMap((q): Row[] => {
      const all = q.branch_id === "all";
      const known = (shell.branches ?? []).filter((b) => isLive(b) && b.tier !== "free");
      const ids = all && known.length ? known.map((b) => b.id) : [q.branch_id];
      return ids.map((branch_id) => ({
        id: 0,
        branch_id,
        ...(all ? { group_id: q.key } : {}),
        kind: q.kind,
        status: q.state === "expired" ? "expired" : q.state === "refused" ? "rejected" : "pending",
        payload: { ...q.payload, ...(q.base_rev === undefined ? {} : { base_rev: q.base_rev }) },
        queued_at: q.created_at,
        local: q,
      }));
    })
    .sort((a, b) => b.queued_at - a.queued_at);
}

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

/**
 * The preview with no signal: each branch as the app last saw it (online or not, and when), and
 * offered only when the plan and setup state it holds say so.
 */
async function lastKnown(d: CommandDraft) {
  const picked = (shell.branches ?? []).filter(
    (b) => d.branch_id === "all" || b.id === d.branch_id,
  );
  const known: Record<string, string | null> = {};
  const targets: OwnerCommandTarget[] = [];
  for (const b of picked) {
    const hit = await getCached<{ branch_online: boolean; last_heartbeat_at?: number }>(
      livePath(b.id),
    );
    known[b.id] = hit?.as_of ?? null;
    const offered = isLive(b) && b.tier !== "free";
    targets.push({
      branch_id: b.id,
      name: b.name,
      online: !!hit?.payload.branch_online,
      ...(hit?.payload.last_heartbeat_at
        ? { last_heartbeat_at: hit.payload.last_heartbeat_at }
        : {}),
      offered,
      ...(offered
        ? {}
        : {
            error: {
              code: b.tier === "free" ? "FEATURE_NOT_ENTITLED" : "UNSUPPORTED",
              message: "",
            },
          }),
    });
  }
  return { known, preview: { targets } };
}

/**
 * What Send posts for a draft, and what still waits for an answer. A plain draft is one post to
 * its branch; a draft with variants posts to each branch the preview offers, once the owner has
 * chosen for every branch the item is missing from.
 */
export function partsOf(d: CommandDraft, targets: OwnerCommandTarget[]) {
  if (!d.variants) {
    const one: Part = { branch_id: d.branch_id, payload: d.payload, key: d.key ?? "" };
    if (d.base_rev !== undefined) one.base_rev = d.base_rev;
    return { send: [one] as Part[], ask: [] as Variant[] };
  }
  const offered = new Set(targets.filter((x) => x.offered).map((x) => x.branch_id));
  const here = d.variants.filter((v) => offered.has(v.branch_id));
  return {
    send: here.filter((v) => !v.ask || d.choices?.[v.branch_id] === "create") as Part[],
    ask: here.filter((v) => v.ask && !d.choices?.[v.branch_id]),
  };
}

/** The owner's answer for a branch the item is missing from. */
export function choose(branchId: string, choice: "create" | "exclude") {
  if (cmd.draft) cmd.draft.choices = { ...cmd.draft.choices, [branchId]: choice };
}

/** Opens the preview for a change. */
export async function propose(draft: CommandDraft) {
  const mine = { ...draft, key: draft.key ?? ulid() };
  cmd.draft = mine;
  cmd.preview = null;
  cmd.known = null;
  cmd.previewFailed = false;
  cmd.previewing = true;
  cmd.sendError = null;
  const res = await send<OwnerCommandPreview>("POST", "/commands/preview", {
    branch_id: mine.branch_id,
    kind: mine.kind,
  });
  const fallback = !res.ok && res.code === "OFFLINE" ? await lastKnown(mine) : null;
  if (cmd.draft?.key !== mine.key) return; // cancelled, or replaced by another change
  cmd.previewing = false;
  if (res.ok) cmd.preview = res.data;
  else if (fallback) {
    cmd.preview = fallback.preview;
    cmd.known = fallback.known;
  } else cmd.previewFailed = true;
}

export function cancel() {
  cmd.draft = null;
  cmd.preview = null;
  cmd.known = null;
  cmd.previewing = false;
  cmd.previewFailed = false;
  cmd.sendError = null;
}

/**
 * Sends the previewed change; with no signal it waits on this device. The cloud skips the
 * branches that can't take it. A change with variants is one post per branch, each with its own
 * key: the ones the cloud took stay taken if a later one fails.
 */
export async function confirm() {
  const d = cmd.draft;
  if (!d || cmd.sending) return;
  const { send: parts, ask } = partsOf(d, cmd.preview?.targets ?? []);
  if (ask.length) return;
  cmd.sending = true;
  cmd.sendError = null;
  const todo = parts.filter((p) => !d.done?.includes(p.key));
  for (const [i, part] of todo.entries()) {
    const res = await post({
      branch_id: part.branch_id,
      idempotency_key: part.key,
      kind: d.kind,
      payload: part.payload,
      ...(part.base_rev === undefined ? {} : { base_rev: part.base_rev }),
    });
    if (res === "declined") {
      // Cancelled at the code prompt: nothing was sent and the preview stays open.
      cmd.sending = false;
      return;
    }
    if (!res.ok) {
      cmd.sending = false;
      if (res.code === "OFFLINE" && part.key) {
        for (const rest of todo.slice(i)) {
          await enqueue({
            key: rest.key,
            branch_id: rest.branch_id,
            kind: d.kind,
            payload: rest.payload,
            base_rev: rest.base_rev,
          });
        }
        cmd.sent++;
        cancel();
        // A response lost on a working line is answered by sending again at once; with no line this stops at the first call.
        void drainQueue();
        return;
      }
      cmd.sendError =
        res.code === "RATE_LIMITED" ? "owner.cmd.send.limited" : "owner.cmd.send.failed";
      return;
    }
    d.done = [...(d.done ?? []), part.key];
    cmd.rows = merge(cmd.rows, res.data.commands);
    cmd.loaded = true;
    takeCodes(res.data);
  }
  cmd.sending = false;
  cmd.sent++;
  cancel();
}

/**
 * Sends a change at once, with no preview (a one-tap action such as a credit reminder): one post to
 * one branch with a fresh key. With no signal it waits on this device like any other change.
 * `"declined"` means the owner cancelled a code prompt.
 */
export async function sendNow(
  branchId: string,
  kind: string,
  payload: Record<string, unknown>,
): Promise<"sent" | "queued" | "failed" | "declined"> {
  const key = ulid();
  const res = await post({ branch_id: branchId, idempotency_key: key, kind, payload });
  if (res === "declined") return "declined";
  if (!res.ok) {
    if (res.code !== "OFFLINE") return "failed";
    await enqueue({ key, branch_id: branchId, kind, payload });
    cmd.sent++;
    void drainQueue();
    return "queued";
  }
  cmd.rows = merge(cmd.rows, res.data.commands);
  cmd.loaded = true;
  cmd.sent++;
  return "sent";
}

/**
 * The `rev` the branch holds now for the row a command changes, from its newest summary; the
 * rev the old command carried when the summary doesn't list the row.
 */
export function currentBaseRev(row: Row): number | undefined {
  const p = row.payload as {
    id?: unknown;
    staff_id?: unknown;
    role_id?: unknown;
    base_rev?: unknown;
  } | null;
  const id = p?.id ?? p?.staff_id ?? p?.role_id;
  const lists = KINDS[row.kind]?.catalogue;
  const catalogue = live[row.branch_id]?.data?.summary?.catalogue;
  if (lists && catalogue && typeof id === "string") {
    for (const list of lists) {
      const found = (catalogue[list] as { id: string; rev: number }[]).find((r) => r.id === id);
      if (found) return found.rev;
    }
  }
  return typeof p?.base_rev === "number" ? p.base_rev : undefined;
}

// --- Step-up MFA (Story 8.25) ---

export type StepMethod = "totp" | "backup_code" | "sms";

/** The code prompt a sensitive change opens when the cloud answers `MFA_REQUIRED`. */
export const stepup = $state({
  open: false,
  method: "totp" as StepMethod,
  sending: false,
  /** Never names the factor: a wrong code reads the same for every method. */
  error: null as "wrong" | "paused" | "failed" | null,
  /** Seconds the cloud asked the owner to wait after five wrong codes. */
  pausedS: 0,
  smsSent: false,
  smsFailed: false,
});

let asking: Promise<boolean> | null = null;
let settle: ((passed: boolean) => void) | null = null;

function closeStepUp(passed: boolean) {
  stepup.open = false;
  const done = settle;
  asking = null;
  settle = null;
  done?.(passed);
}

/** Opens the code prompt; resolves `true` once step-up passed, `false` if the owner cancelled. */
export function requestStepUp(): Promise<boolean> {
  asking ??= new Promise<boolean>((resolve) => {
    settle = resolve;
    Object.assign(stepup, {
      open: true,
      method: "totp",
      sending: false,
      error: null,
      pausedS: 0,
      smsSent: false,
      smsFailed: false,
    });
  });
  return asking;
}

/** Esc or Cancel: nothing is sent and a queued change stays held. */
export const declineStepUp = () => closeStepUp(false);

export function pickMethod(method: StepMethod) {
  stepup.method = method;
  stepup.error = null;
}

/** Checks the code with the cloud; on success the prompt closes and the change goes on. */
export async function submitStepUp(code: string) {
  if (!stepup.open || stepup.sending) return;
  const prompt = asking;
  stepup.sending = true;
  stepup.error = null;
  const res = await send<null>("POST", "/session/stepup", { method: stepup.method, code });
  // The owner cancelled (and maybe opened a new prompt) while the code was being checked.
  if (asking !== prompt) return;
  stepup.sending = false;
  if (res.ok) return closeStepUp(true);
  if (res.code === "RATE_LIMITED") {
    stepup.pausedS = res.retryAfter ?? 900;
    stepup.error = "paused";
  } else stepup.error = res.code === "TOTP_INVALID" ? "wrong" : "failed";
}

/** Texts the owner a code (only offered when text-message codes are on). */
export async function sendStepUpSms() {
  stepup.smsFailed = false;
  const res = await send<unknown>("POST", "/session/stepup/sms", {});
  if (res.ok || res.code === "RATE_LIMITED") {
    // A second tap inside the resend wait still has a code on its way.
    pickMethod("sms");
    stepup.smsSent = true;
  } else stepup.smsFailed = true;
}

/**
 * Posts a command; when the cloud wants a step-up the code prompt opens and the same body, with
 * the same idempotency key, is sent again once it passes. `"declined"`: the owner cancelled.
 */
async function post(body: Record<string, unknown>): Promise<Sent<OwnerCommandPosted> | "declined"> {
  let res = await send<OwnerCommandPosted>("POST", "/commands", body);
  // Asked again if the proof lapsed while the owner was typing: each round needs a fresh code.
  while (!res.ok && res.code === "MFA_REQUIRED") {
    if (!(await requestStepUp())) return "declined";
    res = await send<OwnerCommandPosted>("POST", "/commands", body);
  }
  return res;
}

/** Send on a held change: the code prompt first, then the change goes (Cancel keeps it held). */
export async function sendHeld(q: Queued) {
  if (!(await requestStepUp())) return;
  q.state = "queued";
  await stored(q);
  await drainQueue();
  // A drain already under way may have passed this change by: go once more.
  if (q.state === "queued") await drainQueue();
}

// --- The queue on this device (Story 8.22) ---

const byCreation = (a: Queued, b: Queued) =>
  a.created_at - b.created_at || (a.key < b.key ? -1 : 1);
const expired = (q: Queued, now = Date.now()) => now - q.created_at > COMMAND_TTL_MS;
const stored = (q: Queued) => queuePut($state.snapshot(q));

/** Codes that say "not now" rather than "no": the change stays queued and the drain stops. */
const TRANSIENT = new Set(["OFFLINE", "RATE_LIMITED", "INTERNAL", "UNAVAILABLE", "AUTH_FAILED"]);

/** Keeps a change on this device, with its key, `base_rev` and creation time. */
async function enqueue(c: Pick<Queued, "key" | "branch_id" | "kind" | "payload" | "base_rev">) {
  if (cmd.queued.some((q) => q.key === c.key)) return;
  const q: Queued = { ...c, cafe: shell.cafe, created_at: Date.now(), state: "queued" };
  if (q.base_rev === undefined) delete q.base_rev;
  await stored(q);
  cmd.queued = [...cmd.queued, q].sort(byCreation);
  cmd.announcement = t("owner.cmd.queued_offline");
}

/** Reads this café's queue from IndexedDB; a change past its 7 days is marked expired, not sent. */
export async function loadQueue() {
  const mine = ((await queueAll<Queued>()) ?? [])
    .filter((q) => q.cafe === shell.cafe)
    .sort(byCreation);
  for (const q of mine) {
    if ((q.state === "queued" || q.state === "held") && expired(q)) {
      q.state = "expired";
      await stored(q);
    }
  }
  cmd.queued = mine;
}

async function remove(key: string) {
  await queueDelete(key);
  cmd.queued = cmd.queued.filter((q) => q.key !== key);
}

async function drain() {
  for (const q of [...cmd.queued].sort(byCreation)) {
    if (q.state !== "queued") continue;
    if (expired(q)) {
      q.state = "expired";
      await stored(q);
      continue;
    }
    const res = await send<OwnerCommandPosted>("POST", "/commands", {
      branch_id: q.branch_id,
      idempotency_key: q.key,
      kind: q.kind,
      payload: q.payload,
      ...(q.base_rev === undefined ? {} : { base_rev: q.base_rev }),
    });
    if (res.ok) {
      await remove(q.key);
      cmd.rows = merge(cmd.rows, res.data.commands);
      cmd.loaded = true;
      takeCodes(res.data);
    } else if (TRANSIENT.has(res.code)) {
      return; // later changes wait behind this one: creation order
    } else if (res.code === "MFA_REQUIRED") {
      // Held until the owner passes a step-up from its Send button; the rest go on.
      q.state = "held";
      await stored(q);
      cmd.announcement = t("owner.stepup.held");
    } else {
      q.state = "refused";
      await stored(q);
    }
  }
}

let draining: Promise<void> | null = null;

/** Posts the queued changes in creation order; a refusal marks that one and the rest go on. */
export const drainQueue = () => (draining ??= drain().finally(() => (draining = null)));

/** Reads the queue, then sends it. */
export async function startQueue() {
  await loadQueue();
  await drainQueue();
}

/**
 * Logout: the café's queued changes stay only when the owner chose to keep them, otherwise they
 * go with the cache.
 */
export async function discardQueue() {
  await Promise.all(cmd.queued.map((q) => queueDelete(q.key)));
  cmd.queued = [];
}

/** A queued change that was refused or expired goes again with a fresh key and time. */
async function requeue(old: Queued, rev: number | undefined) {
  await remove(old.key);
  await enqueue({
    key: ulid(),
    branch_id: old.branch_id,
    kind: old.kind,
    payload: old.payload,
    base_rev: rev,
  });
  await drainQueue();
}

/**
 * Retry (rejected) or Resend (expired): a new command with a new key and the current `base_rev`,
 * to this branch only. The old row stays in the history; with no signal the new one is queued.
 * A queued row that was refused or expired is replaced by the new queued change.
 */
export async function again(row: Row) {
  const k = rowKey(row);
  if (cmd.again[k] === "sending") return;
  cmd.again[k] = "sending";
  const b = branchOf(row.branch_id);
  if (b) await fetchLive(b);
  const { base_rev: _stale, ...fields } = (row.payload ?? {}) as Record<string, unknown>;
  const rev = currentBaseRev(row);
  if (row.local) {
    await requeue(row.local, rev);
    delete cmd.again[k];
    return;
  }
  const key = ulid();
  const res = await post({
    branch_id: row.branch_id,
    idempotency_key: key,
    kind: row.kind,
    payload: fields,
    ...(rev === undefined ? {} : { base_rev: rev }),
  });
  if (res === "declined") {
    delete cmd.again[k];
    return;
  }
  if (!res.ok && res.code === "OFFLINE") {
    await enqueue({
      key,
      branch_id: row.branch_id,
      kind: row.kind,
      payload: fields,
      base_rev: rev,
    });
    delete cmd.again[k];
    return;
  }
  if (!res.ok) {
    cmd.again[k] = "failed";
    return;
  }
  delete cmd.again[k];
  cmd.rows = merge(cmd.rows, res.data.commands);
  takeCodes(res.data);
}
