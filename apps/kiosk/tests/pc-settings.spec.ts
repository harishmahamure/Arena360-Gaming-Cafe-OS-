import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import {
  changeOf,
  clampSpeed,
  keepOf,
  modesToList,
  panelOf,
  parseMode,
} from "../src/lib/pc-settings";

// Story 6.35: the PC settings panel in the account menu. The Tauri shell is stubbed in the page:
// the test emits its `pc-screen`, `attract` and `hub-status` events as `@tauri-apps/api/event`
// delivers them, queues the answers the service would give (`__answers`, by command) and reads
// back what the page asked (`__calls`). What is applied on the PC, what the Hub saves and the 15 s
// revert clock are the service's and the Hub's (Rust tests); the countdown here follows the
// `revert_ms` the service reports.

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

async function open(page: Page) {
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
            case commands.pcSettings:
            case commands.pcSettingsSet:
            case commands.pcSettingsKeep:
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
  await emit(page, names.events.hubStatus, { state: "connected" });
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

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

const PREPAID = { username: "asha", remaining_ms: 6_120_000, balance_minor: 25_050 };
const GUEST = { guest_no: 3 };

const panelBody = (over: Record<string, unknown> = {}) => ({
  mouse_speed: 10,
  display_mode: "1920x1080@60",
  display_modes: ["1920x1080@60", "1920x1080@144"],
  audio_outputs: ["Speakers", "Headset"],
  audio_output: "Speakers",
  keyboard_layouts: [
    { id: "04090409", name: "English (United States)" },
    { id: "04070407", name: "German (Germany)" },
  ],
  keyboard_layout: "04090409",
  ...over,
});
const ok = (body: unknown): Answer => ({ ok: true, body });

async function openPanel(page: Page, s: Record<string, unknown>, ...panels: Answer[]) {
  await open(page);
  await emit(page, names.events.pcScreen, {
    locked: false,
    reason: null,
    session: { device_name: "PC 7", elapsed_ms: 0, ...s },
  });
  await expect(page.getByTestId("shell")).toBeVisible();
  await answer(page, names.commands.pcSettings, ...(panels.length ? panels : [ok(panelBody())]));
  await page.getByTestId("account-chip").click();
  await page.getByTestId("account-row-pc-settings").click();
  await expect(page.getByTestId("pc-settings")).toBeVisible();
}

const panel = (page: Page) => page.getByTestId("pc-settings");

test("the PC settings row opens the panel with this PC's current values and choices", async ({
  page,
}) => {
  await openPanel(page, PREPAID);
  await expect(page.getByTestId("pc-settings-mouse")).toHaveValue("10");
  await expect(page.getByTestId("pc-settings-mouse")).toHaveAttribute("min", "1");
  await expect(page.getByTestId("pc-settings-mouse")).toHaveAttribute("max", "20");
  await expect(page.getByTestId("pc-settings-mouse-value")).toHaveText("10 of 20");
  const display = page.getByTestId("pc-settings-display");
  await expect(display).toHaveValue("1920x1080@60");
  await expect(display.locator("option")).toHaveText([
    "1920 × 1080 · 60 Hz",
    "1920 × 1080 · 144 Hz",
  ]);
  await expect(page.getByTestId("pc-settings-audio")).toHaveValue("Speakers");
  await expect(page.getByTestId("pc-settings-audio").locator("option")).toHaveText([
    "Speakers",
    "Headset",
  ]);
  await expect(page.getByTestId("pc-settings-layout")).toHaveValue("04090409");
  await expect(page.getByTestId("pc-settings-layout").locator("option")).toHaveText([
    "English (United States)",
    "German (Germany)",
  ]);
  await expect(page.getByTestId("pc-settings-note")).toHaveText(
    "Your choices are saved to your profile and follow you to any PC.",
  );
  // Focus lands on the first control.
  await expect(page.getByTestId("pc-settings-mouse")).toBeFocused();
  expect(await serious(page)).toEqual([]);
  // Back returns to the rows.
  await panel(page).getByRole("button", { name: "Back" }).click();
  await expect(page.getByTestId("account-row-pc-settings")).toBeVisible();
});

test("a guest gets the panel too, told that the choices last for this session only", async ({
  page,
}) => {
  await openPanel(page, GUEST);
  await expect(page.getByTestId("pc-settings-note")).toHaveText(
    "Guest choices last for this session only.",
  );
});

test("a mouse speed, sound output and keyboard layout change is sent at once, one setting each", async ({
  page,
}) => {
  await openPanel(page, PREPAID);
  await answer(
    page,
    names.commands.pcSettingsSet,
    ok({ saved: true }),
    ok({ saved: true }),
    ok({ saved: true }),
  );
  await answer(
    page,
    names.commands.pcSettings,
    ok(panelBody({ mouse_speed: 15 })),
    ok(panelBody({ mouse_speed: 15, audio_output: "Headset" })),
    ok(panelBody({ mouse_speed: 15, audio_output: "Headset", keyboard_layout: "04070407" })),
  );
  const slider = page.getByTestId("pc-settings-mouse");
  await slider.fill("15");
  await expect(page.getByTestId("pc-settings-mouse-value")).toHaveText("15 of 20");
  await page.getByTestId("pc-settings-audio").selectOption("Headset");
  await expect(page.getByTestId("pc-settings-audio")).toHaveValue("Headset");
  await page.getByTestId("pc-settings-layout").selectOption("04070407");
  await expect(page.getByTestId("pc-settings-layout")).toHaveValue("04070407");
  const sent = (await calls(page))
    .filter(([cmd]) => cmd === names.commands.pcSettingsSet)
    .map(([, args]) => args);
  expect(sent).toEqual([
    { patch: { mouse_speed: 15 } },
    { patch: { audio_output: "Headset" } },
    { patch: { keyboard_layout: "04070407" } },
  ]);
  await expect(page.getByTestId("pc-settings-line")).toHaveText("");
});

test("a new display mode waits for Keep this, and keeping it sends the keep", async ({ page }) => {
  await openPanel(page, PREPAID);
  await answer(page, names.commands.pcSettingsSet, ok({ saved: false, revert_ms: 15000 }));
  await answer(
    page,
    names.commands.pcSettings,
    ok(panelBody({ display_mode: "1920x1080@144", revert_ms: 15000 })),
  );
  await page.getByTestId("pc-settings-display").selectOption("1920x1080@144");
  await expect(page.getByTestId("pc-settings-keep-box")).toBeVisible();
  await expect(page.getByTestId("pc-settings-countdown")).toHaveText(
    /^Keep this display mode\? It goes back in 1[45] s\.$/,
  );
  expect((await calls(page)).find(([cmd]) => cmd === names.commands.pcSettingsSet)?.[1]).toEqual({
    patch: { display_mode: "1920x1080@144" },
  });
  expect(await serious(page)).toEqual([]);
  await answer(page, names.commands.pcSettingsKeep, ok({ saved: true }));
  await answer(page, names.commands.pcSettings, ok(panelBody({ display_mode: "1920x1080@144" })));
  await page.getByRole("button", { name: "Keep this" }).click();
  await expect(page.getByTestId("pc-settings-keep-box")).toHaveCount(0);
  await expect(page.getByTestId("pc-settings-display")).toHaveValue("1920x1080@144");
  expect((await calls(page)).filter(([cmd]) => cmd === names.commands.pcSettingsKeep)).toHaveLength(
    1,
  );
  await expect(page.getByTestId("pc-settings-line")).toHaveText("");
});

test("a display mode that is not kept goes back when the countdown ends", async ({ page }) => {
  await openPanel(page, PREPAID);
  await answer(page, names.commands.pcSettingsSet, ok({ saved: false, revert_ms: 1500 }));
  await answer(
    page,
    names.commands.pcSettings,
    // After the pick, the service still counts down; then it has put the old mode back.
    ok(panelBody({ display_mode: "1920x1080@144", revert_ms: 1500 })),
    ok(panelBody()),
  );
  await page.getByTestId("pc-settings-display").selectOption("1920x1080@144");
  await expect(page.getByTestId("pc-settings-keep-box")).toBeVisible();
  await expect(page.getByTestId("pc-settings-keep-box")).toHaveCount(0, { timeout: 8000 });
  await expect(page.getByTestId("pc-settings-display")).toHaveValue("1920x1080@60");
  await expect(page.getByTestId("pc-settings-line")).toHaveText(
    "⚠ The display went back to what it was.",
  );
  // Nothing was kept, so nothing was sent to keep.
  expect((await calls(page)).filter(([cmd]) => cmd === names.commands.pcSettingsKeep)).toEqual([]);
});

test("with the counter unreachable the change stays and the panel says so", async ({ page }) => {
  await openPanel(page, PREPAID);
  await answer(page, names.commands.pcSettingsSet, { ok: false, code: "HUB_SLOW", body: null });
  await answer(page, names.commands.pcSettings, ok(panelBody({ mouse_speed: 18 })));
  await page.getByTestId("pc-settings-mouse").fill("18");
  await expect(page.getByTestId("pc-settings-line")).toHaveText(
    "⚠ Counter is reconnecting — try again shortly.",
  );
  // The panel shows what the PC is really set to.
  await expect(page.getByTestId("pc-settings-mouse-value")).toHaveText("18 of 20");
});

test("a choice this PC cannot use, and a failed change, say so in place", async ({ page }) => {
  await openPanel(page, PREPAID);
  await answer(
    page,
    names.commands.pcSettingsSet,
    { ok: false, code: "VALIDATION_FAILED", body: null },
    { ok: false, code: "INTERNAL", body: null },
  );
  await answer(page, names.commands.pcSettings, ok(panelBody()), ok(panelBody()));
  await page.getByTestId("pc-settings-audio").selectOption("Headset");
  await expect(page.getByTestId("pc-settings-line")).toHaveText("⚠ This PC can't use that choice.");
  await page.getByTestId("pc-settings-layout").selectOption("04070407");
  await expect(page.getByTestId("pc-settings-line")).toHaveText(
    "⚠ Couldn't change that — try again.",
  );
});

test("a PC with no settings control says so", async ({ page }) => {
  await openPanel(page, PREPAID, { ok: false, code: "UNSUPPORTED", body: null });
  await expect(page.getByTestId("pc-settings-unavailable")).toHaveText(
    "PC settings aren't available on this PC.",
  );
});

test("a PC with no sound output or keyboard layout says none were found", async ({ page }) => {
  await openPanel(
    page,
    PREPAID,
    ok(
      panelBody({
        audio_outputs: [],
        audio_output: null,
        keyboard_layouts: [],
        keyboard_layout: null,
      }),
    ),
  );
  await expect(page.getByTestId("pc-settings-audio-none")).toHaveText("None found on this PC");
  await expect(page.getByTestId("pc-settings-layout-none")).toHaveText("None found on this PC");
});

// ---- The service's answers, read in Node ----

test("the service's answers read into outcomes", () => {
  const body = panelBody();
  expect(panelOf({ ok: true, body })).toEqual({ kind: "ok", panel: body });
  for (const bad of [
    null,
    { ok: false, code: "UNSUPPORTED", body: null },
    { ok: true, body: { ...body, mouse_speed: "fast" } },
    { ok: true, body: { ...body, display_modes: [1] } },
    { ok: true, body: { ...body, keyboard_layouts: [{ id: 1 }] } },
  ]) {
    expect(panelOf(bad)).toEqual({ kind: "unavailable" });
  }

  expect(changeOf({ ok: true, body: { saved: true } })).toEqual({
    kind: "ok",
    saved: true,
    revertMs: null,
  });
  expect(changeOf({ ok: true, body: { saved: false, revert_ms: 15000 } })).toEqual({
    kind: "ok",
    saved: false,
    revertMs: 15000,
  });
  expect(changeOf({ ok: false, code: "HUB_SLOW", body: null })).toEqual({ kind: "reconnecting" });
  expect(changeOf({ ok: false, code: "VALIDATION_FAILED", body: null })).toEqual({
    kind: "not_offered",
  });
  expect(changeOf({ ok: false, code: "INTERNAL", body: null })).toEqual({ kind: "failed" });
  expect(changeOf({ ok: true, body: {} })).toEqual({ kind: "failed" });
  expect(changeOf(null)).toEqual({ kind: "failed" });

  expect(keepOf({ ok: true, body: { saved: true } })).toEqual({ kind: "ok", saved: true });
  expect(keepOf({ ok: false, code: "NOT_FOUND", body: null })).toEqual({ kind: "gone" });
  expect(keepOf({ ok: false, code: "HUB_SLOW", body: null })).toEqual({ kind: "reconnecting" });
  expect(keepOf(null)).toEqual({ kind: "failed" });
});

test("modes are split into numbers, listed with the current one, and speeds stay in Windows' range", () => {
  expect(parseMode("2560x1440@144")).toEqual({ width: 2560, height: 1440, hz: 144 });
  expect(parseMode("big")).toBeNull();
  const p = panelBody({ display_mode: "1280x720@60", display_modes: ["1920x1080@60"] });
  expect(modesToList(p as never)).toEqual(["1280x720@60", "1920x1080@60"]);
  expect(modesToList(panelBody() as never)).toEqual(["1920x1080@60", "1920x1080@144"]);
  expect([clampSpeed(0), clampSpeed(21), clampSpeed(7.4), clampSpeed(Number.NaN)]).toEqual([
    1, 20, 7, 1,
  ]);
});
