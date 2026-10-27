import type {
  IntentPayload,
  PauseReason,
  PaymentRow,
  PcAction,
  Permission,
} from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { waitForAck } from "./admin-unlock";
import { dropElevation, newIdempotencyKey, submitIntent } from "./auth";
import { offerUndo } from "./undo.svelte";

export type Job = {
  payload: IntentPayload;
  key: string;
  rev?: number;
  track: "cmd" | "exit" | "none";
  waiting: MessageKey;
  done: MessageKey;
};

export type Cmd = {
  job: Job;
  state: "waiting" | "done" | "failed" | "refused";
  refusal?: MessageKey;
  detail?: string;
};

export type StartChoice =
  | { mode: "postpaid" }
  | { mode: "pack"; pack_id: string }
  | { mode: "pass"; pack_id: string }
  | { mode: "block"; plan_id: string }
  | { mode: "wallet" };

export type ExtendChoice = {
  minutes: number;
  source: "wallet" | "pack" | "credit";
  pack_id?: string;
};

export const cmds: Record<string, Cmd | undefined> = $state({});

const DONE: Record<PcAction, MessageKey> = {
  lock: "hub.sheet.done.lock",
  unlock: "hub.sheet.done.unlock",
  message: "hub.sheet.done.message",
  restart: "hub.sheet.done.restart",
  shutdown: "hub.sheet.done.shutdown",
};

const REFUSALS: Record<string, MessageKey> = {
  STALE: "hub.settings.branch.stale",
  GROUP_KIND_MISMATCH: "hub.groups.kind_mismatch",
  PERMISSION_DENIED: "hub.refusal.permission",
  PC_BUSY: "hub.session.busy",
  "PC_BUSY:device.power": "hub.session.power_busy",
  PC_UNAVAILABLE: "hub.session.unavailable",
  NO_RATE: "hub.session.no_rate",
  DEVICE_UNASSIGNED: "hub.session.unassigned",
  "STALE:session.end": "hub.session.end_stale",
  "STALE:session.pause": "hub.session.pause_stale",
  "STALE:session.resume": "hub.session.pause_stale",
  "VALIDATION_FAILED:session.pause": "hub.session.pause_refused",
  "VALIDATION_FAILED:session.resume": "hub.session.resume_refused",
  "VALIDATION_FAILED:session.end": "hub.session.ended",
  PAYMENT_SHORT: "hub.settle.short",
  INSUFFICIENT_BALANCE: "hub.settle.insufficient",
  CREDIT_LIMIT: "hub.settle.credit_limit",
  OVERRIDE_USED_TODAY: "hub.settle.override_used",
  REFERENCE_REQUIRED: "hub.settle.reference",
  "STALE:session.settle": "hub.settle.stale",
  PACK_EXPIRED: "hub.session.pack_expired",
  PLAN_NOT_FOR_GROUP: "hub.session.pack_group",
  PASS_OUTSIDE_WINDOW: "hub.session.window_closed",
  "INSUFFICIENT_BALANCE:session.start": "hub.session.wallet_short",
  PAST_CLOSING: "hub.extend.past_closing",
  CREDIT_PAUSED: "hub.plans.sell.credit_paused",
  "STALE:session.extend": "hub.session.pause_stale",
  "VALIDATION_FAILED:session.extend": "hub.extend.refused",
  "NOT_FOUND:session.extend": "hub.extend.refused",
};

const job = (payload: IntentPayload, rest: Partial<Job> & Pick<Job, "done">): Job => ({
  payload,
  key: newIdempotencyKey(),
  track: "cmd",
  waiting: "hub.sheet.waiting",
  ...rest,
});

export const screenJob = (device_id: string, action: PcAction, text?: string) =>
  job({ kind: "device.screen", device_id, action, text }, { done: DONE[action] });

export const powerJob = (device_id: string, action: PcAction) =>
  job({ kind: "device.power", device_id, action }, { done: DONE[action] });

export const startJob = (
  device_id: string,
  player_id?: string,
  manual?: boolean,
  choice: StartChoice = { mode: "postpaid" },
) =>
  job(
    {
      kind: "session.start",
      device_id,
      mode: choice.mode,
      player_id,
      pack_id: choice.mode === "pack" || choice.mode === "pass" ? choice.pack_id : undefined,
      plan_id: choice.mode === "block" ? choice.plan_id : undefined,
    },
    { track: manual ? "none" : "cmd", done: "hub.sheet.done.started" },
  );

export const extendJob = (session_id: string, choice: ExtendChoice) =>
  job(
    {
      kind: "session.extend",
      session_id,
      minutes: choice.minutes,
      source: choice.source,
      pack_id: choice.source === "pack" ? choice.pack_id : undefined,
    },
    { track: "none", done: "hub.sheet.done.extended" },
  );

export const endJob = (session_id: string, rev: number) =>
  job({ kind: "session.end", session_id }, { track: "none", rev, done: "hub.sheet.done.ended" });

export const pauseJob = (session_id: string, rev: number, reason: PauseReason, note?: string) =>
  job(
    { kind: "session.pause", session_id, reason, note },
    { track: "none", rev, done: "hub.sheet.done.paused" },
  );

export const resumeJob = (session_id: string, rev: number) =>
  job(
    { kind: "session.resume", session_id },
    { track: "none", rev, done: "hub.sheet.done.resumed" },
  );

export const settleJob = (order_id: string, payments: PaymentRow[], rev: number) =>
  job(
    { kind: "session.settle", order_id, payments },
    { track: "none", rev, done: "hub.sheet.done.settled" },
  );

export const adminUnlockJob = (device_id: string) =>
  job(
    { kind: "device.admin_unlock", device_id },
    {
      track: "exit",
      waiting: "hub.floor.remote_unlock_waiting",
      done: "hub.floor.remote_unlock_done",
    },
  );

export const maintenanceJob = (device_id: string, rev?: number) =>
  job(
    { kind: "device.maintenance", device_id, on: false },
    { track: "none", rev, done: "hub.sheet.done.saved" },
  );

export const assignJob = (device_id: string, group_id: string, rev?: number) =>
  job(
    { kind: "device.assign", device_id, group_id },
    { track: "none", rev, done: "hub.sheet.done.saved" },
  );

export const PAUSE_LABELS: Record<PauseReason, MessageKey> = {
  player_stepped_away: "hub.pause.reason.player_stepped_away",
  hardware_issue: "hub.pause.reason.hardware_issue",
  power_cut: "hub.pause.reason.power_cut",
  other: "hub.pause.reason.other",
};

export const MESSAGE_MAX = 140;

export const permissionFor = (kind: IntentPayload["kind"]): Permission =>
  kind === "session.start" || kind === "session.end"
    ? kind
    : kind === "session.pause" || kind === "session.resume"
      ? "session.pause"
      : kind === "session.extend"
        ? "session.extend"
        : kind === "session.end.reverted"
          ? "session.end"
          : kind === "session.settle" || kind === "session.settle.reverted"
            ? "pos.sell"
            : kind === "credit.collect" || kind === "credit.collect.reverted"
              ? "credit.collect"
              : kind === "wallet.topup" || kind === "wallet.topup.reverted"
                ? "wallet.topup"
                : kind === "device.screen"
                  ? "device.screen"
                  : kind === "device.power" || kind === "device.admin_unlock"
                    ? "device.remote"
                    : "device.manage";

export type Sent = "sent" | "pin_required" | "signed_out";

export async function send(
  deviceId: string,
  j: Job,
  online: boolean,
  ticket?: string,
): Promise<Sent> {
  if (j.track !== "none" && !online) {
    if (ticket) void dropElevation(ticket);
    cmds[deviceId] = { job: j, state: "failed" };
    return "sent";
  }
  cmds[deviceId] = { job: j, state: "waiting" };
  const settle = (state: Cmd["state"], refusal?: MessageKey, detail?: string) => {
    if (cmds[deviceId]?.job.key === j.key) cmds[deviceId] = { job: j, state, refusal, detail };
  };
  const outcome = await submitIntent(j.payload, j.key, j.rev, { elevationTicket: ticket });
  if (outcome.kind === "signed_out") {
    delete cmds[deviceId];
    return "signed_out";
  }
  if (outcome.kind === "error" && outcome.error.code === "PIN_REQUIRED") {
    delete cmds[deviceId];
    return "pin_required";
  }
  if (outcome.kind === "unreachable") settle("failed");
  else if (outcome.kind === "error") {
    const { code, field, message } = outcome.error;
    const offline = code === "VALIDATION_FAILED" && field === "device_id";
    const extend = j.payload.kind === "session.extend";
    const text =
      (code === "VALIDATION_FAILED" || code === "NOT_FOUND") &&
      (field === "pack_id" || field === "plan_id") &&
      !extend
        ? "hub.session.pack_refused"
        : (REFUSALS[[code, j.payload.kind].join(":")] ?? REFUSALS[code]);
    const detail =
      ((code === "INSUFFICIENT_BALANCE" || code === "PASS_OUTSIDE_WINDOW") &&
        j.payload.kind === "session.start") ||
      (extend &&
        (code === "INSUFFICIENT_BALANCE" ||
          (code === "VALIDATION_FAILED" && (field === "pack_id" || field === "source"))))
        ? message
        : undefined;
    settle(offline ? "failed" : "refused", text ?? "hub.settings.failed", detail);
  } else if (j.track === "none") {
    offerUndo(outcome.result);
    settle("done");
  } else {
    const id =
      j.track === "cmd"
        ? j.key
        : outcome.result.applied.changes.find((c) => c.tbl === "device_admin_exits")?.row_id;
    settle(id && (await waitForAck(id)) ? "done" : "failed");
  }
  return "sent";
}
