import { expect, test } from "@playwright/test";
import { canInstall, openSessions, parseUpdateStatus } from "../src/lib/update";

// The install rule of Story 4.16: only a downloaded update, only with no session active, paused
// or in grace.

test("active, paused and grace sessions are open; ended ones are not", () => {
  expect(openSessions(undefined)).toBe(0);
  expect(openSessions([])).toBe(0);
  expect(
    openSessions([
      { state: "active" },
      { state: "paused" },
      { state: "grace" },
      { state: "ended" },
    ]),
  ).toBe(3);
  expect(openSessions([{ state: "ended" }, { state: "settled" }])).toBe(0);
});

test("an update installs only when it is ready and nothing is open", () => {
  expect(canInstall("ready", 0)).toBe(true);
  expect(canInstall("ready", 1)).toBe(false);
  expect(canInstall("downloading", 0)).toBe(false);
  expect(canInstall("none", 0)).toBe(false);
});

test("an unknown status from the shell is none", () => {
  expect(parseUpdateStatus("ready")).toBe("ready");
  expect(parseUpdateStatus("downloading")).toBe("downloading");
  expect(parseUpdateStatus("installed")).toBe("none");
  expect(parseUpdateStatus(undefined)).toBe("none");
});
