import type {
  IntentPayload,
  PauseReason,
  PaymentRow,
  PcAction,
  Permission,
  ReasonCode,
} from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { waitForAck } from "./admin-unlock";
import { dropElevation, newIdempotencyKey, submitIntent } from "./auth";
import { offerUndo } from "./undo.svelte";

export type Job = {
  payload: IntentPayload;
  key: string;
  rev?: number;
  reason?: ReasonCode;
  track: "cmd" | "exit" | "none";
  moves_to?: string;
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

/** Set when a transfer is filed under its new device, so the floor opens that tile's sheet. */
export const moved: { n: number; deviceId: string | null } = $state({ n: 0, deviceId: null });

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
  "STALE:session.force_end": "hub.session.end_stale",
  "VALIDATION_FAILED:session.force_end": "hub.force.refused",
  "NOT_FOUND:session.force_end": "hub.force.refused",
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
  "STALE:session.transfer": "hub.session.pause_stale",
  "VALIDATION_FAILED:session.transfer": "hub.transfer.refused",
  "NOT_FOUND:session.transfer": "hub.transfer.refused",
  "PC_UNAVAILABLE:session.transfer": "hub.transfer.unavailable",
  "NO_RATE:session.transfer": "hub.transfer.no_rate",
  "DEVICE_UNASSIGNED:session.transfer": "hub.transfer.unassigned",
  "GROUP_KIND_MISMATCH:session.transfer": "hub.transfer.kind_mismatch",
  "PLAN_NOT_FOR_GROUP:session.transfer": "hub.transfer.plan_group",
  "STALE:session.group": "hub.group.stale",
  "VALIDATION_FAILED:session.group": "hub.group.refused",
  "NOT_FOUND:session.group": "hub.group.refused",
  "STALE:group.payer": "hub.group.stale",
  "VALIDATION_FAILED:group.payer": "hub.group.payer_refused",
  "NOT_FOUND:group.payer": "hub.group.payer_refused",
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
  group_with?: string,
) =>
  job(
    {
      kind: "session.start",
      device_id,
      mode: choice.mode,
      group_with,
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

export const transferJob = (session_id: string, device_id: string, rev: number) =>
  job(
    { kind: "session.transfer", session_id, device_id },
    { track: "cmd", rev, moves_to: device_id, done: "hub.sheet.done.transferred" },
  );

export const groupJob = (session_id: string, member_ids: string[], rev: number) =>
  job(
    { kind: "session.group", session_id, member_ids },
    { track: "none", rev, done: "hub.sheet.done.grouped" },
  );

export const payerJob = (
  group_bill_id: string,
  payer: { player_id?: string; guest_no?: number },
  rev: number,
) =>
  job(
    { kind: "group.payer", group_bill_id, ...payer },
    { track: "none", rev, done: "hub.sheet.done.payer" },
  );

export const endJob = (session_id: string, rev: number) =>
  job({ kind: "session.end", session_id }, { track: "none", rev, done: "hub.sheet.done.ended" });

export const FORCE_REASONS: ReasonCode[] = ["rule_violation", "hardware_issue", "closing", "other"];

export const FORCE_LABELS: Record<string, MessageKey> = {
  rule_violation: "reason.rule_violation",
  hardware_issue: "reason.hardware_issue",
  closing: "reason.closing",
  other: "reason.other",
};

export const forceEndJob = (session_id: string, rev: number, reason: ReasonCode, note?: string) =>
  job(
    { kind: "session.force_end", session_id, note },
    { track: "none", rev, reason, done: "hub.sheet.done.force_ended" },
  );

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
    : kind === "session.force_end" || kind === "session.force_end.reverted"
      ? "session.force_end"
      : kind === "session.group"
        ? "session.start"
        : kind === "group.payer"
          ? "pos.sell"
          : kind === "session.pause" || kind === "session.resume"
            ? "session.pause"
            : kind === "session.extend"
              ? "session.extend"
              : kind === "session.transfer"
                ? "session.transfer"
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
  let at = deviceId;
  cmds[at] = { job: j, state: "waiting" };
  const settle = (state: Cmd["state"], refusal?: MessageKey, detail?: string) => {
    if (cmds[at]?.job.key === j.key) cmds[at] = { job: j, state, refusal, detail };
  };
  const outcome = await submitIntent(j.payload, j.key, j.rev, {
    elevationTicket: ticket,
    reason: j.reason,
  });
  if (outcome.kind === "signed_out") {
    delete cmds[at];
    return "signed_out";
  }
  if (outcome.kind === "error" && outcome.error.code === "PIN_REQUIRED") {
    delete cmds[at];
    return "pin_required";
  }
  if (outcome.kind === "unreachable") settle("failed");
  else if (outcome.kind === "error") {
    const { code, field, message } = outcome.error;
    const transfer = j.payload.kind === "session.transfer";
    const offline = code === "VALIDATION_FAILED" && field === "device_id" && !transfer;
    const extend = j.payload.kind === "session.extend";
    const text =
      (code === "VALIDATION_FAILED" || code === "NOT_FOUND") &&
      (field === "pack_id" || field === "plan_id") &&
      !extend
        ? "hub.session.pack_refused"
        : (REFUSALS[[code, j.payload.kind].join(":")] ?? REFUSALS[code]);
    const grouping =
      (code === "VALIDATION_FAILED" || code === "NOT_FOUND") &&
      (field === "group_with" || field === "member_ids");
    const detail =
      grouping ||
      ((code === "INSUFFICIENT_BALANCE" || code === "PASS_OUTSIDE_WINDOW") &&
        j.payload.kind === "session.start") ||
      (extend &&
        (code === "INSUFFICIENT_BALANCE" ||
          (code === "VALIDATION_FAILED" && (field === "pack_id" || field === "source"))))
        ? message
        : undefined;
    settle(offline ? "failed" : "refused", text ?? "hub.settings.failed", detail);
  } else if (j.track === "none") {
    offerUndo(outcome.result, "settle", j.payload.kind === "session.force_end" ? "forced" : "end");
    settle("done");
  } else {
    if (j.moves_to && j.moves_to !== at) {
      delete cmds[at];
      at = j.moves_to;
      cmds[at] = { job: j, state: "waiting" };
      moved.deviceId = at;
      moved.n += 1;
    }
    const id =
      j.track === "cmd"
        ? j.key
        : outcome.result.applied.changes.find((c) => c.tbl === "device_admin_exits")?.row_id;
    settle(id && (await waitForAck(id)) ? "done" : "failed");
  }
  return "sent";
}
