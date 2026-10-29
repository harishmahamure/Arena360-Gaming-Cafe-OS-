import type {
  ControllerResult,
  IntentPayload,
  PauseReason,
  PaymentRow,
  PcAction,
  Permission,
  ReasonCode,
  BusinessBuyer,
  ManualDiscount,
  ReceiptChoice,
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
  cleanup_retry: "hub.sheet.done.cleanup_retry",
  cleanup_checked: "hub.sheet.done.cleanup_checked",
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
  CONTROLLERS_UNCHECKED: "hub.console.mark_first",
  "STALE:controller.check": "hub.console.check_refused",
  "VALIDATION_FAILED:controller.check": "hub.console.check_refused",
  "NOT_FOUND:controller.check": "hub.console.check_refused",
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
  COUPON_INVALID: "hub.settle.coupon.refused",
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
  "VALIDATION_FAILED:order.line_served": "hub.order.line_refused",
  "NOT_FOUND:order.line_served": "hub.order.line_refused",
  "OUT_OF_STOCK:order.line_served": "hub.order.serve_short",
  "VALIDATION_FAILED:order.ready": "hub.order.ready_refused",
  "NOT_FOUND:order.ready": "hub.order.ready_refused",
  "VALIDATION_FAILED:order.line_cancel": "hub.order.line_refused",
  "NOT_FOUND:order.line_cancel": "hub.order.line_refused",
  "VALIDATION_FAILED:order.void": "hub.order.void_refused",
  "NOT_FOUND:order.void": "hub.order.void_refused",
  "STALE:order.void": "hub.order.void_refused",
  "ORDER_SETTLED:order.void": "hub.order.void_settled",
};

const job = (payload: IntentPayload, rest: Partial<Job> & Pick<Job, "done">): Job => ({
  payload,
  key: newIdempotencyKey(),
  track: "cmd",
  waiting: "hub.sheet.waiting",
  ...rest,
});

export const screenJob = (device_id: string, action: PcAction) =>
  job({ kind: "device.screen", device_id, action }, { done: DONE[action] });

/** A staff message to one PC (Story 6.16); the kiosk acks it like any PC command. */
export const messageJob = (device_id: string, text: string) =>
  job({ kind: "device.message", scope: "seat", device_id, text }, { done: DONE.message });

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

export const controllerJob = (session_id: string, result: ControllerResult) =>
  job(
    { kind: "controller.check", session_id, result },
    {
      track: "none",
      done:
        result === "returned"
          ? "hub.sheet.done.controllers_returned"
          : "hub.sheet.done.controllers_missing",
    },
  );

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

export const settleJob = (
  order_id: string,
  payments: PaymentRow[],
  rev: number,
  receipt?: ReceiptChoice,
  business?: BusinessBuyer,
  coupon?: string,
  discount?: ManualDiscount,
) =>
  job(
    { kind: "session.settle", order_id, payments, receipt, business, coupon, discount },
    { track: "none", rev, done: "hub.sheet.done.settled" },
  );

/** Marks a placed kitchen line served: its held stock is taken. */
export const servedJob = (line_id: string) =>
  job({ kind: "order.line_served", line_id }, { track: "none", done: "hub.sheet.done.served" });

/** Marks an order ready for the player (Story 6.33); the kiosk shows the seat or pick-up copy. */
export const readyJob = (order_id: string) =>
  job({ kind: "order.ready", order_id }, { track: "none", done: "hub.sheet.done.ready" });

/** The reasons a placed line is cancelled for, in the order the sheet offers them. */
export const CANCEL_REASONS: ReasonCode[] = ["changed_mind", "mistake", "other"];

export const CANCEL_LABELS: Record<string, MessageKey> = {
  changed_mind: "reason.changed_mind",
  mistake: "reason.mistake",
  other: "reason.other",
};

/** Cancels an unserved line; `made` answers "Was it already made?". */
export const cancelLineJob = (line_id: string, made: boolean, reason: ReasonCode) =>
  job(
    { kind: "order.line_cancel", line_id, made },
    { track: "none", reason, done: "hub.sheet.done.line_cancelled" },
  );

/** The reasons an unsettled order is voided for, in the order the sheet offers them. */
export const VOID_REASONS: ReasonCode[] = ["mistake", "changed_mind", "other"];

export const VOID_LABELS: Record<string, MessageKey> = {
  mistake: "hub.order.void.mistake",
  changed_mind: "hub.order.void.changed_mind",
  other: "hub.order.void.other",
};

/** Voids an unsettled session order; `note` is the text of an `other` reason. */
export const voidJob = (order_id: string, reason: ReasonCode, note?: string) =>
  job(
    { kind: "order.void", order_id, note },
    { track: "none", reason, done: "hub.sheet.done.order_voided" },
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

/** Whether `text` is a staff message: 1 to 140 characters once trimmed, none a control character. */
export const messageValid = (text: string): boolean => {
  const body = text.trim();
  const length = [...body].length;
  return length >= 1 && length <= MESSAGE_MAX && !/[\p{Cc}​-‏‪-‮⁦-⁩﻿]/u.test(body);
};

export const permissionFor = (kind: IntentPayload["kind"]): Permission =>
  kind === "session.start" || kind === "session.end"
    ? kind
    : kind === "controller.check"
      ? "session.end"
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
                    : kind === "session.settle" ||
                        kind === "session.settle.reverted" ||
                        kind === "order.place" ||
                        kind === "order.place.reverted" ||
                        kind === "order.line_served" ||
                        kind === "order.ready" ||
                        kind === "order.line_cancel"
                      ? "pos.sell"
                      : kind === "order.void"
                        ? "pos.void"
                        : kind === "credit.collect" || kind === "credit.collect.reverted"
                          ? "credit.collect"
                          : kind === "wallet.topup" || kind === "wallet.topup.reverted"
                            ? "wallet.topup"
                            : kind === "device.screen" || kind === "device.message"
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
