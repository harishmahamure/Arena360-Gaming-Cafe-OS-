import type { IntentPayload, PaymentRow, PcAction, Permission } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { waitForAck } from "./admin-unlock";
import { dropElevation, newIdempotencyKey, submitIntent } from "./auth";

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
  "VALIDATION_FAILED:session.end": "hub.session.ended",
  PAYMENT_SHORT: "hub.settle.short",
  REFERENCE_REQUIRED: "hub.settle.reference",
  "STALE:session.settle": "hub.settle.stale",
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

export const startJob = (device_id: string, player_id?: string, manual?: boolean) =>
  job(
    { kind: "session.start", device_id, mode: "postpaid", player_id },
    { track: manual ? "none" : "cmd", done: "hub.sheet.done.started" },
  );

export const endJob = (session_id: string, rev: number) =>
  job({ kind: "session.end", session_id }, { track: "none", rev, done: "hub.sheet.done.ended" });

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

export const MESSAGE_MAX = 140;

export const permissionFor = (kind: IntentPayload["kind"]): Permission =>
  kind === "session.start" || kind === "session.end"
    ? kind
    : kind === "session.settle"
      ? "pos.sell"
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
  const settle = (state: Cmd["state"], refusal?: MessageKey) => {
    if (cmds[deviceId]?.job.key === j.key) cmds[deviceId] = { job: j, state, refusal };
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
    const { code, field } = outcome.error;
    const offline = code === "VALIDATION_FAILED" && field === "device_id";
    const text = REFUSALS[[code, j.payload.kind].join(":")] ?? REFUSALS[code];
    settle(offline ? "failed" : "refused", text ?? "hub.settings.failed");
  } else if (j.track === "none") settle("done");
  else {
    const id =
      j.track === "cmd"
        ? j.key
        : outcome.result.applied.changes.find((c) => c.tbl === "device_admin_exits")?.row_id;
    settle(id && (await waitForAck(id)) ? "done" : "failed");
  }
  return "sent";
}
