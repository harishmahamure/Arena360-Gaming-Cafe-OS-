// Input idleness for the Hub UI's idle lock. One `setTimeout` per idle period, rescheduled on
// input (at most once a second). The period's length is read when the period starts, so a new
// `idle_lock_minutes` value applies from the next period and never shortens the one counting.
// Timers stall while the machine sleeps, so waking (`visibilitychange`, `focus`, `pageshow`)
// compares the wall clock with the last reset and locks at once if the period is over; a wall
// clock that went backwards restarts the period from now.

/** The window events that count as input. */
export const IDLE_EVENTS = /** @type {const} */ ([
  "keydown",
  "pointerdown",
  "pointermove",
  "wheel",
  "touchstart",
]);

/** Window events after which the wall clock is checked (the machine may have slept). */
export const WAKE_EVENTS = /** @type {const} */ (["visibilitychange", "focus", "pageshow"]);

/** Input resets the countdown at most this often. */
export const THROTTLE_MS = 1000;

/**
 * @typedef {object} IdleTimers
 * @property {() => number} now
 * @property {(fn: () => void, ms: number) => unknown} setTimeout
 * @property {(id: unknown) => void} clearTimeout
 */

/** @type {IdleTimers} */
const REAL_TIMERS = {
  now: () => Date.now(),
  setTimeout: (fn, ms) => globalThis.setTimeout(fn, ms),
  clearTimeout: (id) => globalThis.clearTimeout(/** @type {number} */ (id)),
};

/**
 * @typedef {object} IdleTracker
 * @property {() => void} start Starts a new idle period and listens for input.
 * @property {() => void} stop Stops listening and cancels the countdown.
 * @property {() => void} activity Input happened (the listeners call this).
 * @property {() => void} check The page woke: lock now if the period is over by the wall clock.
 * @property {() => number} periodMs The current period's length.
 */

/**
 * Calls `onIdle` once `minutes()` minutes pass with no input. After it fires the tracker is
 * stopped; `start()` begins again (after an unlock).
 *
 * @param {{
 *   minutes: () => number,
 *   onIdle: () => void,
 *   target?: Pick<EventTarget, "addEventListener" | "removeEventListener"> | null,
 *   timers?: IdleTimers,
 * }} options
 * @returns {IdleTracker}
 */
export function createIdleTracker({ minutes, onIdle, target = null, timers = REAL_TIMERS }) {
  /** @type {unknown} */
  let timer = null;
  let running = false;
  let lastReset = -Infinity;
  let period = 0;

  const cancel = () => {
    if (timer !== null) timers.clearTimeout(timer);
    timer = null;
  };

  const lengthMs = () => {
    const m = Number(minutes());
    return (Number.isFinite(m) && m > 0 ? m : 5) * 60_000;
  };

  const fire = () => {
    if (!running) return;
    stop();
    onIdle();
  };

  const begin = () => {
    cancel();
    lastReset = timers.now();
    period = lengthMs();
    timer = timers.setTimeout(fire, period);
  };

  const activity = () => {
    if (!running) return;
    const now = timers.now();
    // A wall clock that went backwards restarts the period rather than freezing the throttle.
    if (now >= lastReset && now - lastReset < THROTTLE_MS) return;
    begin();
  };

  const check = () => {
    if (!running) return;
    const now = timers.now();
    if (now < lastReset) begin();
    else if (now - lastReset >= period) fire();
  };

  const listener = () => activity();
  const wake = () => check();

  function start() {
    if (running) stop();
    running = true;
    for (const name of IDLE_EVENTS) {
      target?.addEventListener(name, listener, { passive: true, capture: true });
    }
    for (const name of WAKE_EVENTS) target?.addEventListener(name, wake, { capture: true });
    begin();
  }

  function stop() {
    running = false;
    cancel();
    for (const name of IDLE_EVENTS) {
      target?.removeEventListener(name, listener, { capture: true });
    }
    for (const name of WAKE_EVENTS) target?.removeEventListener(name, wake, { capture: true });
  }

  return { start, stop, activity, check, periodMs: () => period };
}
