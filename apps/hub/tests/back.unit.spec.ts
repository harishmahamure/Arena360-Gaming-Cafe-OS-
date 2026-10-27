import { expect, test } from "@playwright/test";
import { BACK_EVENT, backDepth, handleBack, listenForBack, pushBack } from "../src/lib/back";

// Unit tests for the Android Back stack (no browser page is used).

test("with nothing registered, Back does nothing", () => {
  expect(backDepth()).toBe(0);
  expect(handleBack()).toBe(false);
});

test("only the topmost handler runs, and the one below runs once it is removed", () => {
  const calls: string[] = [];
  const off1 = pushBack(() => calls.push("more"));
  const off2 = pushBack(() => calls.push("sheet"));
  expect(handleBack()).toBe(true);
  expect(calls).toEqual(["sheet"]);
  off2();
  expect(handleBack()).toBe(true);
  expect(calls).toEqual(["sheet", "more"]);
  off1();
  expect(handleBack()).toBe(false);
  expect(calls).toEqual(["sheet", "more"]);
  expect(backDepth()).toBe(0);
});

test("a handler that stays registered runs on every Back", () => {
  let n = 0;
  const off = pushBack(() => n++);
  handleBack();
  handleBack();
  expect(n).toBe(2);
  off();
});

test("a swallowing handler (the lock screen) hides the ones below it", () => {
  const calls: string[] = [];
  const offSheet = pushBack(() => calls.push("sheet"));
  const offLock = pushBack(() => {});
  expect(handleBack()).toBe(true);
  expect(calls).toEqual([]);
  offLock();
  handleBack();
  expect(calls).toEqual(["sheet"]);
  offSheet();
});

test("removing a handler twice, or out of order, leaves the others alone", () => {
  const calls: string[] = [];
  const offA = pushBack(() => calls.push("a"));
  const offB = pushBack(() => calls.push("b"));
  offA();
  offA();
  expect(backDepth()).toBe(1);
  handleBack();
  expect(calls).toEqual(["b"]);
  offB();
});

test("the same function registered twice is two layers", () => {
  const calls: number[] = [];
  const fn = () => calls.push(1);
  const off1 = pushBack(fn);
  const off2 = pushBack(fn);
  off2();
  expect(backDepth()).toBe(1);
  handleBack();
  expect(calls).toEqual([1]);
  off1();
});

test("the shell's window event runs the top handler", () => {
  const target = new EventTarget();
  listenForBack(target);
  let n = 0;
  const off = pushBack(() => n++);
  target.dispatchEvent(new Event(BACK_EVENT));
  expect(n).toBe(1);
  off();
  target.dispatchEvent(new Event(BACK_EVENT));
  expect(n).toBe(1);
});
