import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import { EXTEND_STEPS, extendOf, extendOptionsOf, sourcesOf } from "../src/lib/extend";

// Story 6.21: Extend from the kiosk. The expanded HUD panel's `extend-options`, Extend on the
// warning toasts and the answers the Hub gives. The Tauri shell is stubbed in the page: the test
// queues what `player_extend_options` and `player_extend` answer, reads back every call, and
// `hud_expand` echoes the `hud-expand` event the real shell emits. Pricing at the live rate, who
// may extend, the charge and the refusals are the Hub's (Rust tests); here we check what the
// player is shown and what the panel asks.

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

async function openHud(page: Page) {
  await page.addInitScript(
    ([commands]: [typeof names.commands]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as unknown[][];
      w.__answers = {} as Record<string, unknown[]>;
      w.__layouts = [] as Array<[number, number]>;
      w.__collapses = 0;
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event: string, id: number) => {
          listeners[event] = (listeners[event] ?? []).filter((x) => x !== id);
        },
      };
      const emitEvent = (event: string, payload: unknown) => {
        for (const id of listeners[event] ?? []) callbacks.get(id)?.({ event, id, payload });
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
            case commands.hudLayout:
              (w.__layouts as Array<[number, number]>).push([
                args.width as number,
                args.height as number,
              ]);
              return null;
            case commands.hudCollapse:
              (w.__collapses as number)++;
              return null;
            case commands.hudExpand:
              (w.__calls as unknown[][]).push([cmd, args ?? null]);
              // The shell opens the panel for a running session and tells the page.
              emitEvent("hud-expand", null);
              return null;
            case commands.playerExtendOptions:
            case commands.playerExtend:
              (w.__calls as unknown[][]).push([cmd, args ?? null]);
              return queued(cmd);
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
        emit: emitEvent,
        listening: (event: string) => (listeners[event] ?? []).length > 0,
      };
    },
    [names.commands] as [typeof names.commands],
  );
  await page.goto("/hud");
  for (const event of [
    names.events.pcScreen,
    names.events.hudExpand,
    names.events.attract,
    names.events.hubStatus,
  ]) {
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
    view: { branch_name: "Arena", state: "ready", locale: INR, rate_minor: 6000, media: [] },
    logo: null,
  });
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

const collapses = (page: Page) =>
  page.evaluate(() => (window as unknown as { __collapses: number }).__collapses);

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

const PREPAID = {
  username: "asha",
  balance_minor: 4_000,
  remaining_ms: 40 * 60_000,
  grace_ms: 120_000,
};

const OPTIONS = {
  quotes: [
    { minutes: 30, price_minor: 3_000 },
    { minutes: 60, price_minor: 6_000 },
    { minutes: 120, price_minor: 12_000 },
  ],
  wallet_minor: 4_000,
  packs: [{ pack_id: "K1", name: "10 hours", minutes_left: 90 }],
  credit_headroom_minor: 20_000,
  closing_label: "11:00 pm",
};

const offered = (over: Record<string, unknown> = {}): Answer => ({
  ok: true,
  body: { ...OPTIONS, ...over },
});
const refused = (code: string): Answer => ({
  ok: true,
  body: { outcome: "refused", code, message: "x" },
});
const added: Answer = { ok: true, body: { outcome: "ok" } };

const panel = (page: Page) => page.getByTestId("hud-panel");
const extendPanel = (page: Page) => page.getByTestId("extend-options");
const steps = (page: Page) => page.getByTestId("extend-step");
const sources = (page: Page) => page.getByTestId("extend-source");
const confirm = (page: Page) => page.getByTestId("extend-confirm");
const line = (page: Page) => page.getByTestId("extend-line");

/** A prepaid session on the HUD, the panel opened as Ctrl+Shift+E does. */
async function expanded(page: Page, ...replies: Answer[]) {
  await openHud(page);
  await answer(page, names.commands.playerExtendOptions, offered());
  if (replies.length) await answer(page, names.commands.playerExtend, ...replies);
  await emit(page, names.events.pcScreen, shown(PREPAID));
  await emit(page, names.events.hudExpand);
  await expect(extendPanel(page)).toBeVisible();
  await expect(steps(page)).toHaveCount(3);
}

// ---- the service's answers ----

test("the options in the service's answer are read and anything else is unreachable", () => {
  const ok = extendOptionsOf({ ok: true, body: OPTIONS });
  expect(ok.kind).toBe("ok");
  expect(extendOptionsOf(null)).toEqual({ kind: "unreachable" });
  expect(extendOptionsOf({ ok: true, body: { quotes: [] } })).toEqual({ kind: "unreachable" });
  expect(extendOptionsOf({ ok: false, code: "HUB_SLOW", body: null })).toEqual({
    kind: "unreachable",
  });
  // The Hub answering no (a pass or a guest) is not the link being down.
  expect(extendOptionsOf({ ok: false, code: "VALIDATION_FAILED", body: null })).toEqual({
    kind: "unavailable",
  });
});

test("an extend's answer is ok, a typed refusal or unreachable", () => {
  expect(extendOf(added)).toEqual({ kind: "ok" });
  expect(extendOf(refused("PAST_CLOSING"))).toEqual({ kind: "refused", code: "PAST_CLOSING" });
  expect(extendOf({ ok: false, code: "BAD_FRAME", body: null })).toEqual({
    kind: "refused",
    code: "BAD_FRAME",
  });
  expect(extendOf({ ok: false, code: "HUB_SLOW", body: null })).toEqual({ kind: "unreachable" });
  expect(extendOf(undefined)).toEqual({ kind: "unreachable" });
  expect(extendOf({ ok: true, body: { outcome: "later" } })).toEqual({ kind: "unreachable" });
});

test("the sources are the wallet, each pack, then credit only when it is enabled", () => {
  const read = extendOptionsOf({ ok: true, body: OPTIONS });
  if (read.kind !== "ok") throw new Error("the options did not read");
  const { options } = read;
  expect(sourcesOf(options).map((s) => s.key)).toEqual(["wallet", "pack:K1", "credit"]);
  expect(sourcesOf({ ...options, credit_headroom_minor: undefined, packs: [] })).toEqual([
    { key: "wallet", source: "wallet" },
  ]);
  expect(EXTEND_STEPS).toEqual([30, 60, 120]);
});

// ---- the panel ----

test("Ctrl+Shift+E on prepaid time opens extend-options with focus on the first option", async ({
  page,
}) => {
  await expanded(page);
  expect(await asked(page)).toEqual([names.commands.playerExtendOptions]);
  await expect(steps(page).locator("input")).toHaveCount(3);
  await expect(steps(page).locator("span:first-of-type")).toHaveText(["+30 min", "+1 h", "+2 h"]);
  // Prices as the Hub quoted them, in the branch's money format.
  await expect(page.getByTestId("extend-price")).toHaveText(["₹30.00", "₹60.00", "₹120.00"]);
  await expect(sources(page)).toHaveText([
    "Wallet · ₹40.00",
    "10 hours · 1h 30m left",
    "Credit · ₹200.00 left",
  ]);
  await expect(steps(page).locator("input").first()).toBeFocused();
  expect(await serious(page)).toEqual([]);
});

test("a postpaid session and a guest never see extend-options", async ({ page }) => {
  await openHud(page);
  await emit(page, names.events.pcScreen, shown({ username: "asha", balance_minor: 4_000 }));
  await emit(page, names.events.hudExpand);
  await expect(panel(page)).toBeVisible();
  await expect(extendPanel(page)).toHaveCount(0);
  await emit(page, names.events.pcScreen, shown({ guest_no: 2 }));
  await expect(extendPanel(page)).toHaveCount(0);
  expect(await asked(page)).toEqual([]);
});

test("a step and a source are chosen and the confirm sends them, naming no session or player", async ({
  page,
}) => {
  await expanded(page, added);
  await expect(confirm(page)).toHaveAttribute("aria-disabled", "true");
  await steps(page).nth(1).locator("input").check();
  await sources(page).nth(1).locator("input").check();
  await expect(confirm(page)).not.toHaveAttribute("aria-disabled", "true");
  await confirm(page).click();
  await expect.poll(() => asked(page)).toContain(names.commands.playerExtend);
  const [, args] = (await calls(page)).at(-1)!;
  expect(args).toEqual({ extend: { minutes: 60, source: "pack", pack_id: "K1" } });
});

test("the wallet is the first source and a wallet extend carries no pack", async ({ page }) => {
  await expanded(page, added);
  await expect(sources(page).first().locator("input")).toBeChecked();
  await steps(page).first().locator("input").check();
  await confirm(page).click();
  await expect.poll(() => asked(page)).toContain(names.commands.playerExtend);
  const [, args] = (await calls(page)).at(-1)!;
  expect(args).toEqual({ extend: { minutes: 30, source: "wallet" } });
});

test("an added extend says 'More time added. GG.', closes the panel and any warning toast", async ({
  page,
}) => {
  await openHud(page);
  await answer(page, names.commands.playerExtendOptions, offered());
  await answer(page, names.commands.playerExtend, added);
  await emit(page, names.events.pcScreen, shown({ ...PREPAID, warning_min: 10 }));
  await expect(page.getByTestId("toast-text")).toHaveText("10 min left. Extend?");
  await emit(page, names.events.hudExpand);
  await steps(page).first().locator("input").check();
  await confirm(page).click();
  await expect(extendPanel(page)).toHaveCount(0);
  await expect(page.getByTestId("toast")).toHaveCount(1);
  await expect(page.getByTestId("toast-text")).toHaveText("More time added. GG.");
  await expect(page.getByTestId("toast")).toHaveAttribute("data-kind", "info");
  await expect.poll(() => collapses(page)).toBeGreaterThan(0);
});

test("a refusal keeps the panel open with its line and the other options usable", async ({
  page,
}) => {
  await expanded(page, refused("PAST_CLOSING"), refused("CREDIT_LIMIT"), added);
  // Each refusal is followed by a fresh read of the options.
  await answer(page, names.commands.playerExtendOptions, offered(), offered());
  await steps(page).nth(2).locator("input").check();
  await confirm(page).click();
  await expect(line(page)).toHaveText("⚠ Can't extend past closing time (11:00 pm).");
  await expect(extendPanel(page)).toBeVisible();
  // Pick another source and try again: nothing was disabled by the refusal.
  await sources(page).nth(2).locator("input").check();
  await confirm(page).click();
  await expect(line(page)).toHaveText("⚠ Credit limit reached, see counter.");
  await steps(page).first().locator("input").check();
  await sources(page).first().locator("input").check();
  await confirm(page).click();
  await expect(extendPanel(page)).toHaveCount(0);
  expect(
    (await calls(page)).filter(([c]) => c === names.commands.playerExtend).map(([, a]) => a),
  ).toEqual([
    { extend: { minutes: 120, source: "wallet" } },
    { extend: { minutes: 120, source: "credit" } },
    { extend: { minutes: 30, source: "wallet" } },
  ]);
});

test("a short wallet names the balance and a short pack names its hours, with no override", async ({
  page,
}) => {
  await expanded(page, refused("INSUFFICIENT_BALANCE"), refused("INSUFFICIENT_BALANCE"));
  await answer(
    page,
    names.commands.playerExtendOptions,
    offered({ wallet_minor: 1_000 }),
    offered({ wallet_minor: 1_000 }),
  );
  await steps(page).nth(1).locator("input").check();
  await confirm(page).click();
  await expect(line(page)).toHaveText("⚠ Balance ₹10.00. Top up at counter.");
  await sources(page).nth(1).locator("input").check();
  await confirm(page).click();
  await expect(line(page)).toHaveText("⚠ That pack only has 1h 30m left.");
  // Only Extend and its steps: no manager PIN or override anywhere on the panel.
  await expect(panel(page).getByText(/override|manager|pin/i)).toHaveCount(0);
});

test("a credit pause and any other refusal are said plainly", async ({ page }) => {
  await expanded(page, refused("CREDIT_PAUSED"), refused("STALE"));
  await answer(page, names.commands.playerExtendOptions, offered(), offered());
  await steps(page).first().locator("input").check();
  await sources(page).nth(2).locator("input").check();
  await confirm(page).click();
  await expect(line(page)).toHaveText("⚠ Credit is paused, see counter.");
  await confirm(page).click();
  await expect(line(page)).toHaveText("⚠ Couldn't add time. Please see the counter.");
});

test("with the Hub unreachable every option is disabled and says so, and nothing is asked", async ({
  page,
}) => {
  await openHud(page);
  await emit(page, names.events.hubStatus, { state: "reconnecting" });
  await emit(page, names.events.pcScreen, shown(PREPAID));
  await emit(page, names.events.hudExpand);
  await expect(extendPanel(page)).toBeVisible();
  await expect(steps(page).locator("input")).toHaveCount(3);
  for (const input of await steps(page).locator("input").all()) await expect(input).toBeDisabled();
  await expect(line(page)).toHaveText("⚠ Counter is reconnecting — try again shortly.");
  await expect(confirm(page)).toHaveAttribute("aria-disabled", "true");
  expect(await asked(page)).toEqual([]);
  expect(await serious(page)).toEqual([]);
});

test("the link coming back lets the panel ask again", async ({ page }) => {
  await openHud(page);
  await emit(page, names.events.hubStatus, { state: "reconnecting" });
  await emit(page, names.events.pcScreen, shown(PREPAID));
  await emit(page, names.events.hudExpand);
  await expect(line(page)).toContainText("reconnecting");
  await answer(page, names.commands.playerExtendOptions, offered());
  await emit(page, names.events.hubStatus, { state: "connected" });
  await expect(page.getByTestId("extend-price")).toHaveText(["₹30.00", "₹60.00", "₹120.00"]);
  await expect(line(page)).toHaveText("");
});

test("a Hub that does not answer the extend says the counter is reconnecting", async ({ page }) => {
  await expanded(page, { ok: false, code: "HUB_SLOW", body: null });
  await steps(page).first().locator("input").check();
  await confirm(page).click();
  await expect(line(page)).toHaveText("⚠ Counter is reconnecting — try again shortly.");
  await expect(extendPanel(page)).toBeVisible();
});

test("a session the Hub says cannot extend (a pass or a block) shows that, not options to pay", async ({
  page,
}) => {
  await openHud(page);
  await answer(page, names.commands.playerExtendOptions, {
    ok: false,
    code: "VALIDATION_FAILED",
    body: null,
  });
  await emit(page, names.events.pcScreen, shown(PREPAID));
  await emit(page, names.events.hudExpand);
  await expect(line(page)).toHaveText("⚠ This session can't be extended.");
  await expect(sources(page)).toHaveCount(0);
  await expect(confirm(page)).toHaveAttribute("aria-disabled", "true");
});

test("Esc closes the panel without asking the Hub to extend", async ({ page }) => {
  await expanded(page);
  await page.keyboard.press("Escape");
  await expect(panel(page)).toHaveCount(0);
  expect(await asked(page)).toEqual([names.commands.playerExtendOptions]);
});

// ---- Extend on the warning toasts ----

test("time and low-balance toasts on a prepaid session show Extend beside Dismiss", async ({
  page,
}) => {
  await openHud(page);
  await emit(
    page,
    names.events.pcScreen,
    shown({ ...PREPAID, warning_min: 10, low_balance_minor: 4_000 }),
  );
  await expect(page.getByTestId("toast")).toHaveCount(2);
  for (const toast of await page.getByTestId("toast").all()) {
    const buttons = toast.getByRole("button");
    await expect(buttons).toHaveText(["Extend", "Dismiss"]);
    await expect(buttons.first()).toHaveAttribute("data-testid", "toast-extend");
  }
  // Extend is the warning button: solid, and the only filled control on the toast.
  const background = (testid: string) =>
    page
      .getByTestId(testid)
      .first()
      .evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(await background("toast-extend")).not.toBe("rgba(0, 0, 0, 0)");
  expect(await serious(page)).toEqual([]);
});

test("Extend on a toast opens the panel with the options, as the key does", async ({ page }) => {
  await openHud(page);
  await answer(page, names.commands.playerExtendOptions, offered());
  await emit(page, names.events.pcScreen, shown({ ...PREPAID, warning_min: 5 }));
  await page.getByTestId("toast-extend").click();
  expect(await asked(page)).toContain(names.commands.hudExpand);
  await expect(extendPanel(page)).toBeVisible();
  await expect(steps(page)).toHaveCount(3);
  await expect(steps(page).locator("input").first()).toBeFocused();
});

test("a postpaid time warning and a guest's have no Extend, only Dismiss", async ({ page }) => {
  await openHud(page);
  await emit(
    page,
    names.events.pcScreen,
    shown({ username: "asha", balance_minor: 4_000, warning_min: 5 }),
  );
  await expect(page.getByTestId("toast")).toHaveCount(1);
  await expect(page.getByTestId("toast").getByRole("button")).toHaveText(["Dismiss"]);
  await expect(page.getByTestId("toast-extend")).toHaveCount(0);
});

test("the extend panel passes axe in the high-contrast theme", async ({ page }) => {
  await expanded(page);
  await emit(page, names.events.theme, { hc: true });
  await expect(extendPanel(page)).toBeVisible();
  expect(await serious(page)).toEqual([]);
});
