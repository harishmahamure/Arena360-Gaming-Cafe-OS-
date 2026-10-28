// The branch's shift as every open Hub view sees it: fed from each (re)loaded snapshot. Cash
// needs an open shift (Story 5.10); UPI, card, wallet and credit never do.

import type { ShiftState } from "@arena/ui/generated";

export const shiftState: {
  /** The shift block of the last snapshot, or `null` when the Hub did not send one. */
  current: ShiftState | null;
} = $state({ current: null });

/** Takes the shift block from a (re)loaded snapshot. */
export function setShift(shift: ShiftState | null | undefined) {
  shiftState.current = shift ?? null;
}

/**
 * Whether cash is refused for want of an open shift. A Hub that sends no shift block (an older
 * one) is never treated as closed: the Hub itself still refuses a cash row with `SHIFT_NOT_OPEN`.
 */
export const cashBlocked = (): boolean =>
  shiftState.current !== null && shiftState.current.open == null;
