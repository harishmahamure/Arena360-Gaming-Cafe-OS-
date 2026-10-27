import type { IntentPayload, Permission, SessionStaff, UiIntentResult } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { invalidate } from "$app/navigation";
import {
  SESSION_DEPENDENCY,
  dropElevation,
  elevate,
  hasPermission,
  newIdempotencyKey,
  submitIntent,
} from "./auth";
import { SNAPSHOT_DEPENDENCY } from "./snapshot";

export type UndoKind = "end" | "forced" | "settle" | "topup" | "collect" | "plan";

export type Undo = {
  kind: UndoKind;
  /** The session id for an End, the order id for a Settle. */
  id: string;
  orderId: string;
  /** The `rev` read after the forward action. */
  rev: number;
  amountMinor: number;
  key: string;
  closesAt: number;
  phase: "ready" | "sending" | "approving" | "closing";
  notice?: MessageKey;
  refused?: boolean;
  error?: MessageKey;
};

export const undo: { current: Undo | null; now: number } = $state({
  current: null,
  now: Date.now(),
});

const NOTICE_MS = 3000;
let tick: ReturnType<typeof setInterval> | undefined;
let closer: ReturnType<typeof setTimeout> | undefined;

const PERMISSION: Record<UndoKind, Permission> = {
  end: "session.end",
  forced: "session.force_end",
  settle: "pos.sell",
  topup: "wallet.topup",
  collect: "credit.collect",
  plan: "wallet.topup",
};

const payloadOf = (u: Undo): IntentPayload =>
  u.kind === "end"
    ? { kind: "session.end.reverted", session_id: u.id }
    : u.kind === "forced"
      ? { kind: "session.force_end.reverted", session_id: u.id }
      : u.kind === "topup"
        ? { kind: "wallet.topup.reverted", order_id: u.id }
        : u.kind === "plan"
          ? { kind: "plan.sell.reverted", order_id: u.id }
          : u.kind === "collect"
            ? { kind: "credit.collect.reverted", order_id: u.id }
            : { kind: "session.settle.reverted", order_id: u.id };

const DONE: Record<UndoKind, MessageKey> = {
  end: "hub.undo.done.end",
  forced: "hub.undo.done.forced",
  settle: "hub.undo.done.settle",
  topup: "hub.undo.done.topup",
  collect: "hub.undo.done.collect",
  plan: "hub.undo.done.plan",
};

export const undoPermission = (u: Undo): Permission => PERMISSION[u.kind];

export function secondsLeft(u: Undo): number {
  return Math.max(0, Math.ceil((u.closesAt - undo.now) / 1000));
}

export function closeUndo(): void {
  clearInterval(tick);
  clearTimeout(closer);
  tick = undefined;
  closer = undefined;
  undo.current = null;
}

function closeSoon(): void {
  clearInterval(tick);
  clearTimeout(closer);
  tick = undefined;
  closer = setTimeout(closeUndo, NOTICE_MS);
}

function open(next: Omit<Undo, "key" | "phase">): void {
  closeUndo();
  undo.now = Date.now();
  undo.current = { ...next, key: newIdempotencyKey(), phase: "ready" };
  tick = setInterval(() => {
    undo.now = Date.now();
    const u = undo.current;
    if (u && u.phase !== "sending" && u.phase !== "closing" && undo.now >= u.closesAt) closeUndo();
  }, 250);
}

/**
 * A committed End or Settle replaces whatever toast was open; the earlier action is final in
 * the UI. With an irreversible effect already recorded for it no toast shows at all.
 */
export function offerUndo(
  result: UiIntentResult,
  settledKind: "settle" | "topup" | "collect" | "plan" = "settle",
  endKind: "end" | "forced" = "end",
): void {
  const { ended, settled } = result;
  const now = Date.now();
  if (ended) {
    if (ended.irreversible) return closeUndo();
    open({
      kind: endKind,
      id: ended.session_id,
      orderId: ended.order_id,
      rev: ended.session_rev,
      amountMinor: ended.total_minor,
      closesAt: now + (ended.undo_until - ended.ended_at),
    });
  } else if (settled && typeof settled.order_rev === "number") {
    if (settled.irreversible) return closeUndo();
    open({
      kind: settledKind,
      id: settled.order_id,
      orderId: settled.order_id,
      rev: settled.order_rev,
      amountMinor: settled.total_minor,
      closesAt: now + (settled.undo_until - settled.settled_at),
    });
  }
}

/** A receipt, drawer kick or invoice number was recorded for the bill: Undo is gone. */
export function effectRecorded(orderId: string): void {
  if (undo.current?.orderId === orderId) closeUndo();
}

const refuse = (u: Undo, notice: MessageKey) => {
  undo.current = { ...u, phase: "closing", notice, refused: true, error: undefined };
  closeSoon();
};

export type UndoResult = "done" | "approval" | "refused" | "retry" | "signed_out" | "gone";

/** Sends the matching `.reverted` intent with the forward `rev`. */
export async function runUndo(staff: SessionStaff | null, ticket?: string): Promise<UndoResult> {
  const u = undo.current;
  if (!u || u.phase === "sending" || u.phase === "closing") return "gone";
  if (!ticket && !hasPermission(staff, PERMISSION[u.kind])) {
    undo.current = { ...u, phase: "approving", error: undefined };
    return "approval";
  }
  undo.current = { ...u, phase: "sending", error: undefined };
  const outcome = await submitIntent(payloadOf(u), u.key, u.rev, { elevationTicket: ticket });
  const now = undo.current;
  if (!now || now.key !== u.key) return "gone";
  if (outcome.kind === "applied") {
    undo.current = {
      ...now,
      phase: "closing",
      notice: DONE[u.kind],
      refused: false,
    };
    closeSoon();
    await invalidate(SNAPSHOT_DEPENDENCY);
    return "done";
  }
  if (outcome.kind === "signed_out") {
    closeUndo();
    await invalidate(SESSION_DEPENDENCY);
    return "signed_out";
  }
  if (outcome.kind === "error") {
    const code = outcome.error.code;
    if (code === "PIN_REQUIRED" || code === "PERMISSION_DENIED") {
      if (ticket) void dropElevation(ticket);
      undo.current = { ...now, phase: "approving", error: undefined };
      return "approval";
    }
    refuse(now, code === "STALE" ? "hub.undo.stale" : "hub.undo.not_allowed");
    return "refused";
  }
  undo.current = { ...now, phase: "ready", error: "hub.undo.failed" };
  return "retry";
}

export type Approval = { ok: true } | { ok: false; message: MessageKey; retryAfterS?: number };

/** The inline PIN of an Undo the signed-in member may not do alone. */
export async function approveUndo(staff: SessionStaff | null, pin: string): Promise<Approval> {
  const u = undo.current;
  if (!u) return { ok: true };
  const outcome = await elevate([{ intent_kind: payloadOf(u).kind }], pin);
  if (outcome.kind === "signed_out") return { ok: false, message: "ui.pin.unreachable" };
  if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" };
  if (outcome.kind === "locked") {
    return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS };
  }
  if (outcome.kind === "unreachable") return { ok: false, message: "ui.pin.unreachable" };
  if (outcome.kind === "error") return { ok: false, message: "ui.pin.failed" };
  await runUndo(staff, outcome.ticket);
  return { ok: true };
}

export function cancelApproval(): void {
  const u = undo.current;
  if (u?.phase === "approving") undo.current = { ...u, phase: "ready" };
}
