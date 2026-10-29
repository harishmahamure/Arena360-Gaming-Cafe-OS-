import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import {
  INFO_TOAST_MS,
  WARNING_KEY,
  clock,
  dropWarning,
  graceMinutes,
  hasContent,
  lifetime,
  live,
  raise,
  raiseWarning,
  staffToastKey,
  type HudToast,
} from "../src/lib/hud";

// The overlay HUD (Story 6.14): the pill, the Ctrl+Shift+E panel and the toasts, drawn by the
// `/hud` page of the HUD window. The Tauri shell is stubbed in the page: the test emits its
// `pc-screen`, `attract` and `hud-expand` events as `@tauri-apps/api/event` delivers them and
// reads back the window sizes and collapses the page reported through `hud_layout` and
// `hud_collapse`. Where the window sits, whether it takes focus and the fullscreen-exclusive
// check are the shell's (Rust unit tests in `arena-kiosk-core::hud`).

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

async function openHud(page: Page) {
  await page.addInitScript(
    ([commands]: [typeof names.commands]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event: string, id: number) => {
          listeners[event] = (listeners[event] ?? []).filter((x) => x !== id);
        },
      };
      w.__layouts = [] as Array<[number, number]>;
      w.__collapses = 0;
      w.__ends = [] as unknown[];
      w.__endAnswers = [] as unknown[];
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
            case commands.hudLayout:
              (w.__layouts as Array<[number, number]>).push([
                args.width as number,
                args.height as number,
              ]);
              return null;
            case commands.hudCollapse:
              (w.__collapses as number)++;
              return null;
            case commands.playerEnd:
              (w.__ends as unknown[]).push(cmd);
              return (w.__endAnswers as unknown[]).shift() ?? null;
            case commands.pcScreen:
            case commands.attract:
            case commands.hubStatus:
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
  await page.goto("/hud");
  for (const event of [names.events.pcScreen, names.events.hudExpand, names.events.attract]) {
    await page.waitForFunction(
      (e) =>
        (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
          e,
        ),
      event,
    );
  }
}

const emit = (page: Page, event: string, payload: unknown = null) =>
  page.evaluate(
    ([e, p]) =>
      (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
        e as string,
        p,
      ),
    [event, payload] as const,
  );

const shown = (session: Record<string, unknown> | null, over: Record<string, unknown> = {}) => ({
  locked: false,
  reason: null,
  session: session && { device_name: "PC 7", elapsed_ms: 0, ...session },
  ...over,
});

const layouts = (page: Page) =>
  page.evaluate(() => (window as unknown as { __layouts: Array<[number, number]> }).__layouts);

const lastLayout = async (page: Page) => (await layouts(page)).at(-1);

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

const attract = (page: Page) => emit(page, names.events.attract, { view: view(), logo: null });

const view = () => ({
  branch_name: "Arena",
  state: "ready",
  locale: INR,
  rate_minor: 12000,
  media: [],
});

test("nothing is drawn, and the window asked to be empty, until a session runs", async ({
  page,
}) => {
  await openHud(page);
  await expect(page.getByTestId("hud-pill")).toHaveCount(0);
  await expect(page.getByTestId("toast")).toHaveCount(0);
  await expect.poll(() => lastLayout(page)).toEqual([0, 0]);
});

test("a profiled player's pill shows time left and the wallet balance in hud-numeric", async ({
  page,
}) => {
  await openHud(page);
  await attract(page);
  await emit(
    page,
    names.events.pcScreen,
    shown({ username: "asha", remaining_ms: 3_725_000, balance_minor: 25_050 }),
  );
  const pill = page.getByTestId("hud-pill");
  await expect(pill).toBeVisible();
  await expect(page.getByTestId("hud-time")).toHaveText(/^1:02:0[0-5]$/);
  await expect(page.getByTestId("hud-balance")).toHaveText("₹250.50");
  await expect(pill).toHaveAttribute("aria-label", /^Time left 1:02:0\d\. Balance ₹250\.50$/);
  // hud-numeric: tabular figures.
  expect(
    await page.getByTestId("hud-time").evaluate((el) => getComputedStyle(el).fontVariantNumeric),
  ).toContain("tabular-nums");
  // The window is told to be exactly as big as the pill.
  const box = (await page.getByTestId("hud").boundingBox())!;
  const reported = (await lastLayout(page))!;
  expect(reported[0]).toBeGreaterThanOrEqual(Math.round(box.width));
  expect(reported[1]).toBeGreaterThanOrEqual(Math.round(box.height));
  expect(Math.round((await pill.boundingBox())!.height)).toBe(56);
  expect(await serious(page)).toEqual([]);
});

test("the pill counts down for prepaid and up for postpaid, following the Hub's ticks", async ({
  page,
}) => {
  await openHud(page);
  await emit(page, names.events.pcScreen, shown({ username: "asha", remaining_ms: 90_000 }));
  await expect(page.getByTestId("hud-time")).toHaveText(/^0:01:(29|30)$/);
  await emit(page, names.events.pcScreen, shown({ username: "asha", remaining_ms: 60_000 }));
  await expect(page.getByTestId("hud-time")).toHaveText(/^0:0(0:59|1:00)$/);

  await emit(page, names.events.pcScreen, shown({ username: "asha", elapsed_ms: 125_000 }));
  await expect(page.getByTestId("hud-time")).toHaveText(/^0:02:0[5-6]$/);
  await expect(page.getByTestId("hud-pill")).toHaveAttribute("aria-label", /^Time played /);
});

test("a guest sees time only, with no balance", async ({ page }) => {
  await openHud(page);
  await attract(page);
  await emit(page, names.events.pcScreen, shown({ guest_no: 3, elapsed_ms: 5_000 }));
  await expect(page.getByTestId("hud-pill")).toBeVisible();
  await expect(page.getByTestId("hud-balance")).toHaveCount(0);
  await expect(page.getByTestId("hud-pill")).not.toContainText("₹");
});

test("a lock covers the pill and the window goes empty; clearing it brings the pill back", async ({
  page,
}) => {
  await openHud(page);
  await emit(page, names.events.pcScreen, shown({ guest_no: 1 }));
  await expect(page.getByTestId("hud-pill")).toBeVisible();
  await emit(
    page,
    names.events.pcScreen,
    shown({ guest_no: 1 }, { locked: true, reason: "paused" }),
  );
  await expect(page.getByTestId("hud-pill")).toHaveCount(0);
  await expect.poll(() => lastLayout(page)).toEqual([0, 0]);
  await emit(page, names.events.pcScreen, shown({ guest_no: 1 }));
  await expect(page.getByTestId("hud-pill")).toBeVisible();
  await emit(page, names.events.pcScreen, shown(null));
  await expect(page.getByTestId("hud-pill")).toHaveCount(0);
  await expect.poll(() => lastLayout(page)).toEqual([0, 0]);
});

test("the pill is solid: no translucency, no blur", async ({ page }) => {
  await openHud(page);
  await emit(page, names.events.pcScreen, shown({ guest_no: 1 }));
  const pill = page.getByTestId("hud-pill");
  await expect(pill).toBeVisible();
  const style = (prop: string) =>
    pill.evaluate((el, p) => getComputedStyle(el).getPropertyValue(p), prop);
  expect(await style("background-color")).toMatch(/^rgb\(/);
  expect(await style("backdrop-filter")).toBe("none");
  expect(await style("opacity")).toBe("1");
});

test("Ctrl+Shift+E expands the pill into the panel with focus inside; Esc collapses it", async ({
  page,
}) => {
  await openHud(page);
  await attract(page);
  await emit(
    page,
    names.events.pcScreen,
    shown({
      username: "asha",
      remaining_ms: 1_800_000,
      balance_minor: 10_000,
      plan_name: "Night owl pass",
    }),
  );
  await expect(page.getByTestId("hud-pill")).toBeVisible();

  await emit(page, names.events.hudExpand);
  const panel = page.getByTestId("hud-panel");
  await expect(panel).toBeVisible();
  await expect(page.getByTestId("hud-pill")).toHaveCount(0);
  await expect(page.getByTestId("hud-panel-time")).toHaveText(/^0:29:5\d$|^0:30:00$/);
  await expect(page.getByTestId("hud-panel-balance")).toHaveText("₹100.00");
  await expect(page.getByTestId("hud-panel-plan")).toHaveText("Night owl pass");
  await expect(panel).toBeFocused();
  // The hud-toast palette: solid, amber border.
  const style = (prop: string) =>
    panel.evaluate((el, p) => getComputedStyle(el).getPropertyValue(p), prop);
  expect(await style("background-color")).toBe("rgb(30, 22, 6)");
  expect(await style("border-top-color")).toBe("rgb(255, 190, 61)");
  expect(await serious(page)).toEqual([]);

  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  await expect(page.getByTestId("hud-pill")).toBeVisible();
  expect(
    await page.evaluate(() => (window as unknown as { __collapses: number }).__collapses),
  ).toBe(1);
});

test("the panel's Close button collapses it too; a postpaid panel names its pay-as-you-go plan", async ({
  page,
}) => {
  await openHud(page);
  await emit(page, names.events.pcScreen, shown({ guest_no: 2, elapsed_ms: 61_000 }));
  await emit(page, names.events.hudExpand);
  await expect(page.getByTestId("hud-panel-plan")).toHaveText(en["player.hud.plan_payg"]);
  await expect(page.getByTestId("hud-panel-balance")).toHaveCount(0);
  await page.getByRole("button", { name: en["player.hud.close"] }).click();
  await expect(page.getByTestId("hud-panel")).toHaveCount(0);
  await expect(page.getByTestId("hud-pill")).toBeVisible();
});

test("a prepaid session without a pack names wallet time as its plan", async ({ page }) => {
  await openHud(page);
  await emit(page, names.events.pcScreen, shown({ username: "asha", remaining_ms: 600_000 }));
  await emit(page, names.events.hudExpand);
  await expect(page.getByTestId("hud-panel-plan")).toHaveText(en["player.hud.plan_wallet"]);
});

test("the hotkey does nothing without a session, and a lock closes an open panel", async ({
  page,
}) => {
  await openHud(page);
  await emit(page, names.events.hudExpand);
  await expect(page.getByTestId("hud-panel")).toHaveCount(0);

  await emit(page, names.events.pcScreen, shown({ guest_no: 1 }));
  await emit(page, names.events.hudExpand);
  await expect(page.getByTestId("hud-panel")).toBeVisible();
  await emit(
    page,
    names.events.pcScreen,
    shown({ guest_no: 1 }, { locked: true, reason: "paused" }),
  );
  await expect(page.getByTestId("hud-panel")).toHaveCount(0);
  await emit(page, names.events.pcScreen, shown({ guest_no: 1 }));
  await expect(page.getByTestId("hud-pill")).toBeVisible();
});

test("a blocked-app toast sits above the pill and the window grows to hold both", async ({
  page,
}) => {
  await openHud(page);
  await emit(page, names.events.pcScreen, shown({ guest_no: 1 }));
  await expect(page.getByTestId("hud-pill")).toBeVisible();
  const before = (await lastLayout(page))!;
  await emit(page, names.events.toast, "player.app.blocked");
  const toast = page.getByTestId("toast");
  await expect(toast).toContainText(en["player.app.blocked"]);
  await expect(toast).toHaveAttribute("data-kind", "warning");
  expect((await toast.boundingBox())!.y + (await toast.boundingBox())!.height).toBeLessThanOrEqual(
    (await page.getByTestId("hud-pill").boundingBox())!.y,
  );
  await expect.poll(async () => (await lastLayout(page))![1]).toBeGreaterThan(before[1]);
  expect((await lastLayout(page))![0]).toBe(640);
  await page.getByRole("button", { name: en["common.dismiss"] }).click();
  await expect(toast).toHaveCount(0);
  await expect.poll(async () => (await lastLayout(page))![1]).toBe(before[1]);
});

test("a toast with no session still shows, alone, and the window empties when it goes", async ({
  page,
}) => {
  await openHud(page);
  await emit(page, names.events.toast, "player.app.blocked");
  await expect(page.getByTestId("toast")).toHaveCount(1);
  await expect.poll(async () => (await lastLayout(page))![0]).toBe(640);
  await page.getByRole("button", { name: en["common.dismiss"] }).click();
  await expect.poll(() => lastLayout(page)).toEqual([0, 0]);
});

test("the toast and the panel pass axe in the high-contrast theme", async ({ page }) => {
  await openHud(page);
  await emit(page, names.events.theme, { hc: true });
  await emit(page, names.events.pcScreen, shown({ guest_no: 1 }));
  await emit(page, names.events.hudExpand);
  await emit(page, names.events.toast, "player.app.blocked");
  await expect(page.getByTestId("hud-panel")).toBeVisible();
  await expect(page.getByTestId("toast")).toBeVisible();
  expect(await serious(page)).toEqual([]);
});

// The toast lifetimes are pure rules (`hud.ts`), tested here without a page.

const toast = (over: Partial<HudToast> = {}): HudToast => ({
  id: 1,
  key: "player.app.blocked",
  kind: "warning",
  until: null,
  ...over,
});

test("an info toast goes after 8 s; a warning stays", () => {
  expect(INFO_TOAST_MS).toBe(8000);
  expect(lifetime("info")).toBe(8000);
  expect(lifetime("warning")).toBeNull();
  const t0 = 1000;
  const list = raise(raise([], "a", "info", t0, 1), "b", "warning", t0, 2);
  expect(live(list, t0 + 7999).map((x) => x.key)).toEqual(["a", "b"]);
  expect(live(list, t0 + 8000).map((x) => x.key)).toEqual(["b"]);
  expect(live(list, t0 + 3_600_000).map((x) => x.key)).toEqual(["b"]);
});

test("the same toast again replaces the one showing and restarts its clock", () => {
  const first = raise([], "a", "info", 0, 1);
  const again = raise(first, "a", "info", 5000, 2);
  expect(again).toHaveLength(1);
  expect(again[0].id).toBe(2);
  expect(live(again, 12_000)).toHaveLength(1);
  expect(live(again, 13_000)).toHaveLength(0);
});

test("the HUD draws something only for a session or a toast", () => {
  expect(hasContent(false, [])).toBe(false);
  expect(hasContent(true, [])).toBe(true);
  expect(hasContent(false, [toast()])).toBe(true);
});

test("the clock reads H:MM:SS and never goes below zero", () => {
  expect(clock(3725)).toBe("1:02:05");
  expect(clock(59.9)).toBe("0:00:59");
  expect(clock(-5)).toBe("0:00:00");
});

test("a staff message toast carries its text under a key of its own, so two stack", () => {
  const one = raise([], staffToastKey(1), "info", 0, 1, "Closing at 11");
  const two = raise(one, staffToastKey(2), "info", 10, 2, "Closing at 11");
  expect(two.map((x) => x.text)).toEqual(["Closing at 11", "Closing at 11"]);
  expect(two.map((x) => x.key)).toEqual(["staff:1", "staff:2"]);
  expect(two[0].until).toBe(INFO_TOAST_MS);
  expect(live(two, INFO_TOAST_MS + 1).map((x) => x.id)).toEqual([2]);
  expect("text" in raise([], "a", "info", 0, 1)).toBe(false);
});

// ---- Story 6.18: "I'm done" in the expanded panel ----

const ends = (page: Page) =>
  page.evaluate(() => (window as unknown as { __ends: unknown[] }).__ends.length);

const endAnswer = (page: Page, answer: unknown) =>
  page.evaluate(
    (a) => (window as unknown as { __endAnswers: unknown[] }).__endAnswers.push(a),
    answer,
  );

async function expandPostpaid(page: Page) {
  await openHud(page);
  await attract(page);
  await emit(page, names.events.pcScreen, shown({ username: "asha", balance_minor: 10_000 }));
  await emit(page, names.events.hudExpand);
  await expect(page.getByTestId("hud-panel")).toBeVisible();
}

test("a postpaid panel offers I'm done, asks in place and ends only on confirm", async ({
  page,
}) => {
  await expandPostpaid(page);
  await expect(page.getByTestId("hud-done")).toHaveText("I'm done");
  await page.getByTestId("hud-done").click();
  await expect(page.getByTestId("hud-done-confirm")).toHaveText(
    /^End your session\? You'll pay at the counter\./,
  );
  await expect(page.getByTestId("hud-done-cancel")).toBeFocused();
  expect(await serious(page)).toEqual([]);
  expect(await ends(page)).toBe(0);
  await endAnswer(page, { ok: true, body: { outcome: "ok" } });
  await page.getByTestId("hud-done-yes").click();
  await expect.poll(() => ends(page)).toBe(1);
});

test("Esc on the I'm done question cancels it and keeps the panel; a second Esc collapses", async ({
  page,
}) => {
  await expandPostpaid(page);
  await page.getByTestId("hud-done").click();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("hud-done-confirm")).toHaveCount(0);
  await expect(page.getByTestId("hud-panel")).toBeVisible();
  await page.getByTestId("hud-done").click();
  await page.getByTestId("hud-done-cancel").click();
  await expect(page.getByTestId("hud-done-confirm")).toHaveCount(0);
  expect(await ends(page)).toBe(0);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("hud-panel")).toHaveCount(0);
});

test("a prepaid panel has no I'm done", async ({ page }) => {
  await openHud(page);
  await attract(page);
  await emit(
    page,
    names.events.pcScreen,
    shown({ username: "asha", remaining_ms: 1_800_000, balance_minor: 10_000 }),
  );
  await emit(page, names.events.hudExpand);
  await expect(page.getByTestId("hud-panel")).toBeVisible();
  await expect(page.getByTestId("hud-done")).toHaveCount(0);
});

test("I'm done with the Hub unreachable sends nothing and says the counter is reconnecting", async ({
  page,
}) => {
  await expandPostpaid(page);
  await emit(page, names.events.hubStatus, { state: "reconnecting" });
  await page.getByTestId("hud-done").click();
  await expect(page.getByTestId("hud-done-line")).toHaveText(
    "⚠ Counter is reconnecting — try again shortly.",
  );
  await expect(page.getByTestId("hud-done-confirm")).toHaveCount(0);
  expect(await ends(page)).toBe(0);
});

test("a refused I'm done keeps the session and says so", async ({ page }) => {
  await expandPostpaid(page);
  await endAnswer(page, { ok: true, body: { outcome: "refused", code: "STALE", message: "x" } });
  await page.getByTestId("hud-done").click();
  await page.getByTestId("hud-done-yes").click();
  await expect(page.getByTestId("hud-done-line")).toHaveText("⚠ Couldn't log out — try again.");
  await expect(page.getByTestId("hud-panel")).toBeVisible();
});

// ---- Story 6.19: time warnings as HUD toasts ----

const prepaid = (warning_min: number | null, over: Record<string, unknown> = {}) =>
  shown({
    username: "asha",
    balance_minor: 10_000,
    remaining_ms: (warning_min ?? 20) * 60_000,
    grace_ms: 120_000,
    warning_min,
    ...over,
  });

test("a 10-minute warning is a toast with the end time, the grace line and the Extend key caps", async ({
  page,
}) => {
  await openHud(page);
  await attract(page);
  await emit(page, names.events.pcScreen, prepaid(10));
  const toast = page.getByTestId("toast");
  await expect(toast).toHaveCount(1);
  await expect(toast).toHaveAttribute("data-kind", "warning");
  await expect(page.getByTestId("toast-text")).toHaveText("10 min left. Extend?");
  await expect(page.getByTestId("toast-detail")).toHaveText(
    /^Time's up at \d{1,2}:\d{2}( [AP]M)?\. You get a 2-min grace after that\.$/,
  );
  await expect(page.getByTestId("toast-keys")).toHaveAccessibleName(en["player.warning.hotkey"]);
  await expect(page.getByTestId("toast-keys").locator("kbd")).toHaveText(["Ctrl", "Shift", "E"]);
  await expect(toast.getByRole("button", { name: en["common.dismiss"] })).toBeVisible();
  expect(await page.evaluate(() => document.activeElement === document.body)).toBe(true);
});

test("a warning without a grace says only when time is up", async ({ page }) => {
  await openHud(page);
  await emit(page, names.events.pcScreen, prepaid(5, { grace_ms: 0 }));
  await expect(page.getByTestId("toast-detail")).toHaveText(
    /^Time's up at \d{1,2}:\d{2}( [AP]M)?\.$/,
  );
  await expect(page.getByTestId("toast-detail")).not.toContainText("grace");
});

test("a postpaid warning shows the title with no end time", async ({ page }) => {
  await openHud(page);
  await emit(page, names.events.pcScreen, shown({ guest_no: 1, warning_min: 5 }));
  await expect(page.getByTestId("toast-text")).toHaveText("5 min left. Extend?");
  await expect(page.getByTestId("toast-detail")).toHaveCount(0);
});

test("a newer warning replaces the older, and it stays until dismissed", async ({ page }) => {
  await openHud(page);
  await emit(page, names.events.pcScreen, prepaid(10));
  await expect(page.getByTestId("toast-text")).toHaveText("10 min left. Extend?");
  await emit(page, names.events.pcScreen, prepaid(5));
  await expect(page.getByTestId("toast")).toHaveCount(1);
  await expect(page.getByTestId("toast-text")).toHaveText("5 min left. Extend?");
  // The same mark on the next tick is not raised again.
  await emit(page, names.events.pcScreen, prepaid(5));
  await expect(page.getByTestId("toast")).toHaveCount(1);
  await page.getByRole("button", { name: en["common.dismiss"] }).click();
  await expect(page.getByTestId("toast")).toHaveCount(0);
  await emit(page, names.events.pcScreen, prepaid(5));
  await expect(page.getByTestId("toast")).toHaveCount(0);
  await emit(page, names.events.pcScreen, prepaid(1));
  await expect(page.getByTestId("toast-text")).toHaveText("1 min left. Extend?");
});

test("time-up, the end of the session or an extension takes the warning away", async ({ page }) => {
  await openHud(page);
  await emit(page, names.events.pcScreen, prepaid(5));
  await expect(page.getByTestId("toast")).toHaveCount(1);
  await emit(page, names.events.pcScreen, prepaid(5, { warning_min: null }));
  await expect(page.getByTestId("toast")).toHaveCount(0);
  await emit(page, names.events.pcScreen, prepaid(1));
  await expect(page.getByTestId("toast")).toHaveCount(1);
  await emit(page, names.events.pcScreen, { ...prepaid(1), locked: true, reason: "times_up" });
  await expect(page.getByTestId("toast")).toHaveCount(0);
  await emit(page, names.events.pcScreen, prepaid(1));
  await emit(page, names.events.pcScreen, shown(null));
  await expect(page.getByTestId("toast")).toHaveCount(0);
});

test("a warning toast passes axe in the high-contrast theme", async ({ page }) => {
  await openHud(page);
  await emit(page, names.events.theme, { hc: true });
  await emit(page, names.events.pcScreen, prepaid(5));
  await expect(page.getByTestId("toast")).toBeVisible();
  expect(await serious(page)).toEqual([]);
});

test("a time warning is one toast under one key and stays until dismissed", () => {
  const first = raiseWarning([], "10 min left. Extend?", "Time's up at 7:23 pm.", 1);
  expect(first).toHaveLength(1);
  expect(first[0]).toMatchObject({ key: WARNING_KEY, kind: "warning", until: null, hotkey: true });
  expect(live(first, 10 * 60_000)).toHaveLength(1);
  const next = raiseWarning(raise(first, "player.app.blocked", "warning", 0, 2), "5 min", null, 3);
  expect(next.map((x) => x.id)).toEqual([2, 3]);
  expect("detail" in next[1]).toBe(false);
  expect(dropWarning(next).map((x) => x.id)).toEqual([2]);
});

test("grace is said in whole minutes, a part of a minute counting as one", () => {
  expect(graceMinutes(0)).toBe(0);
  expect(graceMinutes(120_000)).toBe(2);
  expect(graceMinutes(90_000)).toBe(2);
});
