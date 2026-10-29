// The volume slider's rules (Story 6.15). Pure, so the control and its tests share them.

/** The arrow keys move the slider by this many percent. */
export const VOLUME_STEP = 5;

/** A whole percent from 0 to 100; anything else becomes the nearest end (NaN: 0). */
export function clampVolume(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(100, Math.max(0, Math.round(value)));
}
