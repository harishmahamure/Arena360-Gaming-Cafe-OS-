import { expect, test } from "@playwright/test";
import { THROTTLE_MS, createIdleTracker } from "../src/lib/idle.js";

// Unit tests for the idle tracker with a fake clock (no browser page is used).

/** A fake clock with `setTimeout`; `advance` runs the timers that come due, in order. */
function fakeTimers() {
  let now = 0;
  let next = 1;
  const pending = new Map<number, { at: number; fn: () => void }>();
  return {
    now: () => now,
    setTimeout: (fn: () => void, ms: number) => {
      const id = next++;
      pending.set(id, { at: now + ms, fn });
      return id;
    },
    clearTimeout: (id: unknown) => void pending.delete(id as number),
    /** Moves the wall clock only (sleep, or a clock change): no timer runs. */
    jump(ms: number) {
      now += ms;
    },
    advance(ms: number) {
      const end = now + ms;
      for (;;) {
        const due = [...pending].filter(([, t]) => t.at <= end).sort((a, b) => a[1].at - b[1].at);
        if (due.length === 0) break;
        const [id, t] = due[0];
        pending.delete(id);
        now = t.at;
        t.fn();
      }
      now = end;
    },
    pending: () => pending.size,
  };
}

const MIN = 60_000;

function setup(minutes = 5) {
  const timers = fakeTimers();
  const settings = { minutes };
  let idle = 0;
  const tracker = createIdleTracker({
    minutes: () => settings.minutes,
    onIdle: () => idle++,
    timers,
  });
  return { timers, settings, tracker, idled: () => idle };
}

test("locks after the idle period with no input", () => {
  const { timers, tracker, idled } = setup();
  tracker.start();
  timers.advance(5 * MIN - 1);
  expect(idled()).toBe(0);
  timers.advance(1);
  expect(idled()).toBe(1);
  // Stopped once it fired: nothing more until it is started again.
  timers.advance(60 * MIN);
  expect(idled()).toBe(1);
  expect(timers.pending()).toBe(0);
});

test("input at 4:59 resets the countdown", () => {
  const { timers, tracker, idled } = setup();
  tracker.start();
  timers.advance(4 * MIN + 59_000);
  tracker.activity();
  timers.advance(5 * MIN - 1);
  expect(idled()).toBe(0);
  timers.advance(1);
  expect(idled()).toBe(1);
});

test("input resets at most once a second, with one timer", () => {
  const { timers, tracker, idled } = setup();
  tracker.start();
  timers.advance(10_000);
  tracker.activity();
  // Within the throttle window: ignored, so the countdown still ends 5 min after 0:10.
  timers.advance(THROTTLE_MS - 1);
  tracker.activity();
  expect(timers.pending()).toBe(1);
  timers.advance(5 * MIN - THROTTLE_MS);
  expect(idled()).toBe(0);
  timers.advance(1);
  expect(idled()).toBe(1);
});

test("a new setting applies from the next period, never shortening the current one", () => {
  const { timers, settings, tracker, idled } = setup(5);
  tracker.start();
  timers.advance(2 * MIN);
  settings.minutes = 1;
  timers.advance(3 * MIN - 1);
  expect(idled()).toBe(0);
  timers.advance(1);
  expect(idled()).toBe(1);
  // After an unlock the next period uses the new value.
  settings.minutes = 10;
  tracker.start();
  expect(tracker.periodMs()).toBe(10 * MIN);
  timers.advance(10 * MIN - 1);
  expect(idled()).toBe(1);
  timers.advance(1);
  expect(idled()).toBe(2);
});

test("stop cancels the countdown and ignores input", () => {
  const { timers, tracker, idled } = setup();
  tracker.start();
  tracker.stop();
  tracker.activity();
  timers.advance(30 * MIN);
  expect(idled()).toBe(0);
  expect(timers.pending()).toBe(0);
});

test("listens to the input events on its target and lets go on stop", () => {
  const timers = fakeTimers();
  const target = new EventTarget();
  let idle = 0;
  const tracker = createIdleTracker({ minutes: () => 1, onIdle: () => idle++, target, timers });
  tracker.start();
  for (const name of ["keydown", "pointerdown", "pointermove", "wheel", "touchstart"]) {
    timers.advance(30_000);
    target.dispatchEvent(new Event(name));
  }
  // Each event reset the 1-minute countdown.
  expect(idle).toBe(0);
  timers.advance(MIN);
  expect(idle).toBe(1);
  tracker.start();
  tracker.stop();
  target.dispatchEvent(new Event("keydown"));
  timers.advance(5 * MIN);
  expect(idle).toBe(1);
});

test("waking after a sleep past the period locks at once", () => {
  const timers = fakeTimers();
  const target = new EventTarget();
  let idle = 0;
  const tracker = createIdleTracker({ minutes: () => 5, onIdle: () => idle++, target, timers });
  tracker.start();
  // Asleep: the timer never ran, but the wall clock moved on.
  timers.jump(4 * MIN);
  target.dispatchEvent(new Event("focus"));
  expect(idle).toBe(0);
  timers.jump(MIN);
  for (const name of ["visibilitychange", "pageshow", "focus"])
    target.dispatchEvent(new Event(name));
  expect(idle).toBe(1);
  expect(timers.pending()).toBe(0);
});

test("a wall clock that goes backwards restarts the period from now", () => {
  const { timers, tracker, idled } = setup();
  tracker.start();
  timers.advance(MIN);
  timers.jump(-10 * MIN);
  // Input right after the jump is not swallowed by the throttle: it restarts the period.
  tracker.activity();
  timers.advance(5 * MIN - 1);
  expect(idled()).toBe(0);
  timers.advance(1);
  expect(idled()).toBe(1);
  tracker.start();
  timers.jump(-MIN);
  tracker.check();
  expect(idled()).toBe(1);
  timers.advance(5 * MIN);
  expect(idled()).toBe(2);
});
