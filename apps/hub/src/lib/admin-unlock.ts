// Remote unlock acknowledgements (Story 1.20). The layout's event stream records every
// `device.admin_unlock_ack`; the floor waits here for the one its unlock minted. An ack can arrive
// before the intent's HTTP answer, so recent acks are kept for a while.

/** How long the floor waits for a kiosk to acknowledge a remote unlock. */
export const ACK_TIMEOUT_MS = 5000;
/** How long an ack is remembered. */
const KEEP_MS = 60_000;

const acked = new Map<string, number>();
const waiters = new Map<string, Array<() => void>>();

/** Records that a kiosk granted `exitId`. */
export function noteAck(exitId: string) {
  const now = Date.now();
  for (const [id, at] of acked) if (now - at > KEEP_MS) acked.delete(id);
  acked.set(exitId, now);
  for (const wake of waiters.get(exitId) ?? []) wake();
  waiters.delete(exitId);
}

/** Resolves `true` once `exitId` is acknowledged, `false` after `timeoutMs`. */
export function waitForAck(exitId: string, timeoutMs = ACK_TIMEOUT_MS): Promise<boolean> {
  if (acked.has(exitId)) return Promise.resolve(true);
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      const list = (waiters.get(exitId) ?? []).filter((w) => w !== wake);
      if (list.length) waiters.set(exitId, list);
      else waiters.delete(exitId);
      resolve(false);
    }, timeoutMs);
    const wake = () => {
      clearTimeout(timer);
      resolve(true);
    };
    waiters.set(exitId, [...(waiters.get(exitId) ?? []), wake]);
  });
}
