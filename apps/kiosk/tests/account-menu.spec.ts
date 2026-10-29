import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import {
  UNLOCK_LOCK_S,
  accountOf,
  endOf,
  recordUnlock,
  unlockLeftS,
  unlockOf,
} from "../src/lib/account";

// Story 6.17: the account menu, Lock and Log out. The Tauri shell is stubbed in the page: the test
// emits its `pc-screen`, `attract` and `hub-status` events as `@tauri-apps/api/event` delivers
// them, queues the answers the service would give (`__answers`, by command) and reads back what
// the page asked (`__calls`). Who is on the session, the wallet, the password check and the
// 60 s pause are the Hub's (Rust tests); the pause countdown here follows `retry_s`.

const INR = {
  currency: "INR",
  exponent: 2,
  symbol: "₹",
  symbol_position: "prefix",
  number_format: "indian",
  tz: "Asia/Kolkata",
  day_start_min: 0,
  fy_start_month: 4,
  pending: null,
};

type Answer = { ok: boolean; code?: string; body: unknown };

async function open(page: Page, hub: "connected" | "reconnecting" = "connected") {
  await page.addInitScript(
    ([commands]: [typeof names.commands]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as unknown[];
      w.__answers = {} as Record<string, unknown[]>;
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event: string, id: number) => {
          listeners[event] = (listeners[event] ?? []).filter((x) => x !== id);
        },
      };
      const queued = (cmd: string) =>
        ((w.__answers as Record<string, unknown[]>)[cmd] ??= []).shift() ?? null;
      w.__TAURI_INTERNALS__ = {
        transformCallback: (cb: (e: unknown) => void) => {
          const id = next++;
          callbacks.set(id, cb);
          return id;
        },
        invoke: async (cmd: string, args: Record<string, unknown>) => {
          switch (cmd) {
            case "plugin:event|listen":
              (listeners[args.event as string] ??= []).push(args.handler as number);
              return args.handler;
            case "plugin:event|unlisten":
              return null;
            case commands.staffMessages:
              return [];
            case commands.volumeGet:
              return { ok: true, body: { percent: 50 } };
            case commands.playerAccount:
            case commands.playerEnd:
            case commands.playerLock:
            case commands.playerUnlock:
            case commands.hudExpand:
              (w.__calls as unknown[]).push([cmd, args ?? null]);
              return queued(cmd);
            case commands.pcScreen:
            case commands.attract:
            case commands.hubStatus:
            case commands.takeRestoreState:
            case commands.reportAppState:
            case commands.heartbeat:
            case commands.adminExitStatus:
            case commands.theme:
              return null;
            default:
              throw new Error(`unexpected command ${cmd}`);
          }
        },
      };
      w.__shell = {
        emit: (event: string, payload: unknown) => {
          for (const id of listeners[event] ?? []) callbacks.get(id)?.({ event, id, payload });
        },
        listening: (event: string) => (listeners[event] ?? []).length > 0,
      };
    },
    [names.commands] as [typeof names.commands],
  );
  await page.goto("/");
  for (const event of [names.events.pcScreen, names.events.hubStatus, names.events.attract]) {
    await page.waitForFunction(
      (e) =>
        (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
          e,
        ),
      event,
    );
  }
  await emit(page, names.events.hubStatus, { state: hub });
  await emit(page, names.events.attract, {
    view: { branch_name: "Arena", state: "ready", locale: INR, rate_minor: 12000, media: [] },
    logo: null,
  });
}

const emit = (page: Page, event: string, payload: unknown) =>
  page.evaluate(
    ([e, p]) =>
      (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
        e as string,
        p,
      ),
    [event, payload] as const,
  );

const answer = (page: Page, cmd: string, ...answers: Answer[]) =>
  page.evaluate(
    ([c, a]) => {
      const q = (window as unknown as { __answers: Record<string, unknown[]> }).__answers;
      (q[c as string] ??= []).push(...(a as unknown[]));
    },
    [cmd, answers] as const,
  );

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __calls: unknown[][] }).__calls);

const asked = (page: Page) => calls(page).then((c) => c.map(([cmd]) => cmd));

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

const shown = (session: Record<string, unknown> | null, over: Record<string, unknown> = {}) => ({
  locked: false,
  reason: null,
  session: session && { device_name: "PC 7", elapsed_ms: 0, ...session },
  ...over,
});

const PREPAID = { username: "asha", remaining_ms: 6_120_000, balance_minor: 25_050 };
const POSTPAID = { username: "asha", balance_minor: 25_050 };
const GUEST = { guest_no: 3 };

const menu = (page: Page) => page.getByTestId("account-menu");
const row = (page: Page, name: string) => page.getByTestId(`account-row-${name}`);

async function session(page: Page, s: Record<string, unknown>) {
  await emit(page, names.events.pcScreen, shown(s));
  await expect(page.getByTestId("shell")).toBeVisible();
}

test("the account chip and the time chip each open the menu with name, balance and time", async ({
  page,
}) => {
  await open(page);
  await session(page, PREPAID);
  await expect(menu(page)).toHaveCount(0);
  await page.getByTestId("account-chip").click();
  await expect(menu(page)).toBeVisible();
  await expect(page.getByTestId("account-menu-name")).toHaveText("asha");
  await expect(page.getByTestId("account-menu-balance")).toHaveText("Balance ₹250.50");
  await expect(page.getByTestId("account-menu-time")).toHaveText(/^Time left 1:41:5\d$/);
  await expect(menu(page).getByRole("button")).toHaveText([
    "Balance",
    "Messages",
    "Account",
    /^Extend/,
    "Lock",
    "Log out",
  ]);
  expect(await serious(page)).toEqual([]);
  // The chip that opened it opens it again after Esc; the time chip works the same.
  await page.keyboard.press("Escape");
  await expect(menu(page)).toHaveCount(0);
  await expect(page.getByTestId("account-chip")).toBeFocused();
  await page.getByTestId("time-chip").click();
  await expect(menu(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("time-chip")).toBeFocused();
});

test("the menu is a keyboard panel: focus lands inside, arrows move and a click outside closes", async ({
  page,
}) => {
  await open(page);
  await session(page, PREPAID);
  await page.getByTestId("account-chip").focus();
  await page.keyboard.press("Enter");
  await expect(row(page, "balance")).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(row(page, "messages")).toBeFocused();
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await expect(row(page, "logout")).toBeFocused();
  await page.getByTestId("shell-logo").click();
  await expect(menu(page)).toHaveCount(0);
});

test("a guest's menu has no Balance and no Lock, and a postpaid chip says time played", async ({
  page,
}) => {
  await open(page);
  await session(page, GUEST);
  await page.getByTestId("account-chip").click();
  await expect(page.getByTestId("account-menu-name")).toHaveText("Guest 3");
  await expect(page.getByTestId("account-menu-balance")).toHaveCount(0);
  await expect(menu(page).getByRole("button")).toHaveText(["Messages", "I'm done", "Log out"]);
  await expect(page.getByTestId("account-menu-time")).toHaveText(/^Time played /);
});

test("Balance shows the wallet and the pack hours with the counter line", async ({ page }) => {
  await open(page);
  await session(page, PREPAID);
  await page.getByTestId("account-chip").click();
  await answer(page, names.commands.playerAccount, {
    ok: true,
    body: {
      balance_minor: 20_000,
      packs: [
        { name: "5 h", minutes_left: 135, expires_at: Date.UTC(2026, 10, 20) },
        { name: "Night pass", expires_at: Date.UTC(2026, 10, 25) },
      ],
    },
  });
  await row(page, "balance").click();
  await expect(page.getByTestId("account-balance-wallet")).toHaveText("Wallet ₹200.00");
  await expect(page.getByTestId("account-balance-packs")).toContainText("5 h: 2h 15m left");
  await expect(page.getByTestId("account-balance-packs")).toContainText("Night pass: valid until");
  await expect(page.getByTestId("account-balance-topups")).toHaveText(
    "Top-ups are accepted at the counter.",
  );
  expect(await serious(page)).toEqual([]);
  // Back returns to the rows.
  await page.getByRole("button", { name: "Back" }).click();
  await expect(row(page, "balance")).toBeVisible();
});

test("Balance with the counter unreachable says so and sends nothing", async ({ page }) => {
  await open(page, "reconnecting");
  await session(page, PREPAID);
  await page.getByTestId("account-chip").click();
  await row(page, "balance").click();
  await expect(page.getByTestId("account-balance-error")).toHaveText(
    "Counter is reconnecting — try again shortly.",
  );
  expect(await asked(page)).toEqual([]);
});

test("Messages closes the menu and opens the messages bell's list", async ({ page }) => {
  await open(page);
  await session(page, PREPAID);
  await page.getByTestId("account-chip").click();
  await row(page, "messages").click();
  await expect(menu(page)).toHaveCount(0);
  await expect(page.getByTestId("messages-panel")).toBeVisible();
});

test("Lock asks the service to cover the screen and the menu closes", async ({ page }) => {
  await open(page);
  await session(page, PREPAID);
  await page.getByTestId("account-chip").click();
  await answer(page, names.commands.playerLock, { ok: true, body: null });
  await row(page, "lock").click();
  await expect(menu(page)).toHaveCount(0);
  expect(await asked(page)).toEqual([names.commands.playerLock]);
});

test("a Lock the service refuses stays in the menu with a line", async ({ page }) => {
  await open(page);
  await session(page, PREPAID);
  await page.getByTestId("account-chip").click();
  await answer(page, names.commands.playerLock, {
    ok: false,
    code: "VALIDATION_FAILED",
    body: null,
  });
  await row(page, "lock").click();
  await expect(page.getByTestId("account-menu-line")).toHaveText("⚠ Couldn't lock — try again.");
});

test("the lock screen is solid with the time still running and opens with the right password", async ({
  page,
}) => {
  await open(page);
  await session(page, PREPAID);
  await emit(
    page,
    names.events.pcScreen,
    shown(PREPAID, { locked: true, reason: "locked_by_player" }),
  );
  await expect(page.getByTestId("pc-locked")).toBeVisible();
  await expect(page.getByRole("heading", { name: "PC locked" })).toBeVisible();
  await expect(page.getByTestId("unlock-password")).toBeFocused();
  expect(await serious(page)).toEqual([]);
  // A wrong password: the field clears, the line says so, the field keeps focus.
  await answer(
    page,
    names.commands.playerUnlock,
    { ok: true, body: { outcome: "rejected" } },
    { ok: true, body: { outcome: "ok" } },
  );
  await page.getByTestId("unlock-password").fill("nope");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("unlock-error")).toContainText("That password isn't right.");
  await expect(page.getByTestId("unlock-password")).toHaveValue("");
  await expect(page.getByTestId("unlock-password")).toBeFocused();
  // Paste is allowed.
  await page.getByTestId("unlock-password").evaluate((el) => {
    const input = el as HTMLInputElement;
    input.focus();
    const data = new DataTransfer();
    data.setData("text/plain", "right-one");
    input.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true }));
    input.value = "right-one";
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await page.keyboard.press("Enter");
  await expect.poll(() => asked(page)).toHaveLength(2);
  expect((await calls(page))[1]).toEqual([names.commands.playerUnlock, { password: "right-one" }]);
  // The service lifts the lock by announcing the screen again.
  await emit(page, names.events.pcScreen, shown(PREPAID));
  await expect(page.getByTestId("pc-locked")).toHaveCount(0);
});

test("five wrong passwords pause the field with a countdown from the Hub's answer", async ({
  page,
}) => {
  await open(page);
  await session(page, PREPAID);
  await emit(
    page,
    names.events.pcScreen,
    shown(PREPAID, { locked: true, reason: "locked_by_player" }),
  );
  const wrong = { ok: true, body: { outcome: "rejected" } };
  await answer(page, names.commands.playerUnlock, wrong, wrong, wrong, wrong, {
    ok: true,
    body: { outcome: "locked", retry_s: 60 },
  });
  for (let n = 0; n < 5; n++) {
    await page.getByTestId("unlock-password").fill("nope");
    await page.keyboard.press("Enter");
    await expect.poll(() => asked(page).then((c) => c.length)).toBe(n + 1);
  }
  await expect(page.getByTestId("unlock-password")).toBeDisabled();
  await expect(page.getByTestId("unlock-paused")).toContainText("Unlock");
  await expect(page.getByTestId("unlock-lock-status")).toContainText(
    "Too many tries. Try again in 0:",
  );
  expect(await serious(page)).toEqual([]);
});

test("the unlock button waits for the counter when the Hub is unreachable", async ({ page }) => {
  await open(page, "reconnecting");
  await session(page, PREPAID);
  await emit(
    page,
    names.events.pcScreen,
    shown(PREPAID, { locked: true, reason: "locked_by_player" }),
  );
  await expect(page.getByRole("button", { name: "Unlock" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await page.getByTestId("unlock-password").fill("x");
  await page.keyboard.press("Enter");
  expect(await asked(page)).toEqual([]);
});

test("Log out with prepaid time left asks in place with the real time, and Stay changes nothing", async ({
  page,
}) => {
  await open(page);
  await session(page, PREPAID);
  await page.getByTestId("account-chip").click();
  await row(page, "logout").click();
  await expect(page.getByTestId("account-logout-confirm")).toHaveText(
    "You still have 1h 42m. Log out anyway?",
  );
  await expect(page.getByRole("button", { name: "Stay" })).toBeFocused();
  expect(await serious(page)).toEqual([]);
  expect(await asked(page)).toEqual([]);
  await page.keyboard.press("Enter");
  await expect(row(page, "logout")).toBeVisible();
  expect(await asked(page)).toEqual([]);
});

test("confirming Log out on prepaid time asks the Hub to end the session once", async ({
  page,
}) => {
  await open(page);
  await session(page, PREPAID);
  await page.getByTestId("account-chip").click();
  await row(page, "logout").click();
  await answer(page, names.commands.playerEnd, { ok: true, body: { outcome: "ok" } });
  await page.getByRole("button", { name: "Log out" }).click();
  await expect(menu(page)).toHaveCount(0);
  expect(await asked(page)).toEqual([names.commands.playerEnd]);
});

test("Log out on a postpaid session ends it at once, with no question", async ({ page }) => {
  await open(page);
  await session(page, POSTPAID);
  await page.getByTestId("account-chip").click();
  await answer(page, names.commands.playerEnd, { ok: true, body: { outcome: "ok" } });
  await row(page, "logout").click();
  await expect(page.getByTestId("account-logout-confirm")).toHaveCount(0);
  await expect(menu(page)).toHaveCount(0);
  expect(await asked(page)).toEqual([names.commands.playerEnd]);
});

test("Log out with the Hub unreachable sends nothing and the row says the counter is reconnecting", async ({
  page,
}) => {
  await open(page, "reconnecting");
  await session(page, POSTPAID);
  await page.getByTestId("account-chip").click();
  await row(page, "logout").click();
  await expect(page.getByTestId("account-menu-line")).toHaveText(
    "⚠ Counter is reconnecting — try again shortly.",
  );
  expect(await asked(page)).toEqual([]);
  expect(await serious(page)).toEqual([]);
});

test("a Hub refusal of Log out keeps the player in the session with a line", async ({ page }) => {
  await open(page);
  await session(page, POSTPAID);
  await page.getByTestId("account-chip").click();
  await answer(page, names.commands.playerEnd, {
    ok: true,
    body: { outcome: "refused", code: "STALE", message: "x" },
  });
  await row(page, "logout").click();
  await expect(page.getByTestId("account-menu-line")).toHaveText("⚠ Couldn't log out — try again.");
  await expect(page.getByTestId("shell")).toBeVisible();
});

test("a postpaid menu lists I'm done above Log out; prepaid has none", async ({ page }) => {
  await open(page);
  await session(page, POSTPAID);
  await page.getByTestId("account-chip").click();
  await expect(menu(page).getByRole("button")).toHaveText([
    "Balance",
    "Messages",
    "Account",
    "Lock",
    "I'm done",
    "Log out",
  ]);
  await page.keyboard.press("Escape");
  await session(page, PREPAID);
  await page.getByTestId("account-chip").click();
  await expect(row(page, "done")).toHaveCount(0);
});

test("I'm done asks in place and ends the session only on confirm", async ({ page }) => {
  await open(page);
  await session(page, POSTPAID);
  await page.getByTestId("account-chip").click();
  await row(page, "done").click();
  await expect(page.getByTestId("account-done-confirm")).toHaveText(
    "End your session? You'll pay at the counter.",
  );
  await expect(page.getByRole("button", { name: "Cancel" })).toBeFocused();
  expect(await serious(page)).toEqual([]);
  expect(await asked(page)).toEqual([]);
  await answer(page, names.commands.playerEnd, { ok: true, body: { outcome: "ok" } });
  await page.getByRole("button", { name: "End session" }).click();
  await expect(menu(page)).toHaveCount(0);
  expect(await asked(page)).toEqual([names.commands.playerEnd]);
});

test("Cancel on the I'm done question changes nothing", async ({ page }) => {
  await open(page);
  await session(page, POSTPAID);
  await page.getByTestId("account-chip").click();
  await row(page, "done").click();
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByTestId("account-done-confirm")).toHaveCount(0);
  await expect(row(page, "done")).toBeVisible();
  expect(await asked(page)).toEqual([]);
});

test("Esc on the I'm done question closes the menu and sends nothing", async ({ page }) => {
  await open(page);
  await session(page, POSTPAID);
  await page.getByTestId("account-chip").click();
  await row(page, "done").click();
  await page.keyboard.press("Escape");
  await expect(menu(page)).toHaveCount(0);
  await expect(page.getByTestId("shell")).toBeVisible();
  expect(await asked(page)).toEqual([]);
});

test("I'm done with the Hub unreachable sends nothing and says the counter is reconnecting", async ({
  page,
}) => {
  await open(page, "reconnecting");
  await session(page, POSTPAID);
  await page.getByTestId("account-chip").click();
  await row(page, "done").click();
  await expect(page.getByTestId("account-menu-line")).toHaveText(
    "⚠ Counter is reconnecting — try again shortly.",
  );
  await expect(page.getByTestId("account-done-confirm")).toHaveCount(0);
  expect(await asked(page)).toEqual([]);
});

// ---- the pure rules ----

test("the service's answers read into outcomes and anything else is unreachable", () => {
  expect(
    accountOf({ ok: true, body: { balance_minor: 5, packs: [{ name: "a", expires_at: 1 }] } }),
  ).toEqual({ kind: "ok", balanceMinor: 5, packs: [{ name: "a", expires_at: 1 }] });
  expect(accountOf({ ok: true, body: { balance_minor: "5", packs: [] } })).toEqual({
    kind: "unreachable",
  });
  expect(accountOf(null)).toEqual({ kind: "unreachable" });
  expect(endOf({ ok: true, body: { outcome: "ok" } })).toEqual({ kind: "ok" });
  expect(endOf({ ok: true, body: { outcome: "refused", message: "m" } })).toEqual({
    kind: "refused",
    message: "m",
  });
  expect(endOf({ ok: false, code: "HUB_SLOW", body: null })).toEqual({ kind: "unreachable" });
  expect(unlockOf({ ok: true, body: { outcome: "rejected" } })).toEqual({ kind: "rejected" });
  expect(unlockOf({ ok: true, body: { outcome: "locked", retry_s: 12.2 } })).toEqual({
    kind: "locked",
    retryS: 13,
  });
  expect(unlockOf({ ok: true, body: { outcome: "locked" } })).toEqual({
    kind: "locked",
    retryS: UNLOCK_LOCK_S,
  });
  expect(unlockOf({ ok: true, body: { outcome: "?" } })).toEqual({ kind: "unreachable" });
});

test("the fifth wrong password pauses for 60 s and the Hub's locked sets its own pause", () => {
  const t0 = 1_000;
  for (let n = 0; n < 4; n++) recordUnlock({ kind: "rejected" }, t0);
  expect(unlockLeftS(t0)).toBe(0);
  recordUnlock({ kind: "unreachable" }, t0);
  recordUnlock({ kind: "rejected" }, t0);
  expect(unlockLeftS(t0)).toBe(UNLOCK_LOCK_S);
  expect(unlockLeftS(t0 + 59_001)).toBe(1);
  expect(unlockLeftS(t0 + 60_000)).toBe(0);
  recordUnlock({ kind: "locked", retryS: 30 }, t0);
  expect(unlockLeftS(t0)).toBe(30);
  recordUnlock({ kind: "ok" }, t0);
  expect(unlockLeftS(t0)).toBe(0);
});

test("prepaid sessions get an Extend row above Lock with the Ctrl+Shift+E key caps", async ({
  page,
}) => {
  await open(page);
  await session(page, PREPAID);
  await page.getByTestId("account-chip").click();
  const extend = row(page, "extend");
  await expect(extend).toBeVisible();
  await expect(page.getByTestId("account-row-extend-keys")).toHaveAttribute(
    "aria-label",
    "Extend with Ctrl+Shift+E",
  );
  await expect(extend.locator("kbd")).toHaveText(["Ctrl", "Shift", "E"]);
  const order = await menu(page)
    .locator("[data-testid^=account-row-]:not([data-testid$=keys])")
    .evaluateAll((els) => els.map((e) => e.getAttribute("data-testid")));
  expect(order.indexOf("account-row-extend")).toBe(order.indexOf("account-row-lock") - 1);
  // Arrow keys reach it between Account and Lock.
  await row(page, "account").focus();
  await page.keyboard.press("ArrowDown");
  await expect(extend).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(row(page, "lock")).toBeFocused();
  expect(await serious(page)).toEqual([]);
});

test("Extend opens the HUD panel as Ctrl+Shift+E does, and the menu closes", async ({ page }) => {
  await open(page);
  await session(page, PREPAID);
  await page.getByTestId("account-chip").click();
  await row(page, "extend").click();
  await expect(menu(page)).toHaveCount(0);
  expect(await asked(page)).toEqual([names.commands.hudExpand]);
});

test("a postpaid session or a guest is not offered Extend", async ({ page }) => {
  await open(page);
  await session(page, POSTPAID);
  await page.getByTestId("account-chip").click();
  await expect(row(page, "extend")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await emit(page, names.events.pcScreen, shown(GUEST));
  await page.getByTestId("account-chip").click();
  await expect(row(page, "extend")).toHaveCount(0);
});
