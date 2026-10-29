import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type {
  AttractView,
  PlanOption,
  PlayerAuthResult,
  PlayerPlansResult,
  PlayerStartResult,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

// The plan picker (Story 6.9): the cards after a login, the muted ones, choosing and starting,
// waiting, the typed refusals, the unreachable Hub, the idle forget and the hand-over to the
// session screen. The Tauri shell is stubbed in the page: `player_plans` and `player_start`
// answer from queues the test fills, and every call is recorded.

const INR = {
  currency: "INR",
  exponent: 2,
  symbol: "₹",
  symbol_position: "before",
  number_format: "indian",
  tz: "Asia/Kolkata",
  day_start_min: 360,
  fy_start_month: 4,
  pending: null,
} as const;

const view = (over: Partial<AttractView> = {}): AttractView => ({
  branch_name: "Level Up Gaming",
  device_name: "PC 7",
  zone: "Main hall",
  state: "ready",
  rate_minor: 12000,
  group_name: "Gaming PCs",
  unpaid_minor: 0,
  locale: { ...INR },
  media: [],
  ...over,
});

type Answer = { ok: boolean; code?: string; body: unknown } | null;
const ASHA: Answer = {
  ok: true,
  body: { outcome: "ok", player: { player_id: "P1", name: "Asha Rao" } } satisfies PlayerAuthResult,
};

const FUTURE = Date.UTC(2027, 2, 4, 6, 30);

const pack = (over: Partial<PlanOption> = {}): PlanOption => ({
  choice: { mode: "pack", pack_id: "K1" },
  name: "10 hour pack",
  selectable: true,
  minutes_left: 150,
  expires_at: FUTURE,
  ...over,
});
const expiredPack = (): PlanOption =>
  pack({
    choice: { mode: "pack", pack_id: "K0" },
    name: "Old pack",
    selectable: false,
    tag: "expired",
    minutes_left: 45,
  });
const pass = (over: Partial<PlanOption> = {}): PlanOption => ({
  choice: { mode: "pass", pack_id: "S1" },
  name: "Night pass",
  selectable: true,
  expires_at: FUTURE,
  window: "22:00–06:00",
  ...over,
});
const block = (over: Partial<PlanOption> = {}): PlanOption => ({
  choice: { mode: "block", plan_id: "B1" },
  name: "Night block",
  selectable: true,
  window: "22:00–06:00",
  ends_label: "06:00",
  price_minor: 20000,
  ...over,
});
const walletCard = (): PlanOption => ({
  choice: { mode: "wallet" },
  name: "",
  selectable: true,
});

const plans = (
  options: PlanOption[],
  balance = 4000,
  rule: NonNullable<PlayerPlansResult["wallet"]>["rule"] = undefined,
): Answer => ({
  ok: true,
  body: {
    wallet: { balance_minor: balance, rate_minor: 12000, ...(rule ? { rule } : {}) },
    options,
  } satisfies PlayerPlansResult,
});

const FULL = (): Answer =>
  plans([pack(), expiredPack(), pass(), block(), walletCard()], 4000, {
    rule_id: "R1",
    name: "Happy hour",
    adjust_kind: "percent_bp",
    adjust_value: -2000,
    rate_minor: 9600,
    until: FUTURE,
  });

const started = (r: PlayerStartResult): Answer => ({ ok: true, body: r });
const OK = started({ outcome: "ok" });
const refused = (code: string): Answer =>
  started({ outcome: "refused", code: code as never, message: "ignored" });
const LOGIN_GONE: Answer = { ok: false, code: "AUTH_FAILED", body: null };

type Opts = { hub?: "connected" | "reconnecting" | null; clock?: boolean };

async function open(page: Page, opts: Opts = {}) {
  const hub = opts.hub === undefined ? "connected" : opts.hub;
  if (opts.clock) await page.clock.install();
  await page.addInitScript(
    ([commands, shown, state]: [typeof names.commands, unknown, string | null]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as unknown[];
      w.__logins = [] as unknown[];
      w.__plans = [] as unknown[];
      w.__starts = [] as unknown[];
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event: string, id: number) => {
          listeners[event] = (listeners[event] ?? []).filter((x) => x !== id);
        },
      };
      let lastPlans: unknown = null;
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
            case commands.attract:
              return shown;
            case commands.hubStatus:
              return state === null ? null : { state };
            case commands.playerLogin: {
              (w.__logins as unknown[]).push(args);
              return {
                ok: true,
                body: { outcome: "ok", player: { player_id: "P1", name: "Asha Rao" } },
              };
            }
            case commands.playerPlans: {
              (w.__calls as unknown[]).push({ cmd: "plans" });
              const queue = w.__plans as unknown[];
              if (queue.length > 0) lastPlans = queue.shift();
              return lastPlans;
            }
            case commands.playerStart: {
              (w.__calls as unknown[]).push({ cmd: "start", args });
              const queue = w.__starts as unknown[];
              return queue.length > 0 ? queue.shift() : null;
            }
            case commands.playerLogout:
              (w.__calls as unknown[]).push({ cmd: "logout" });
              return { ok: true, body: null };
            case commands.takeRestoreState:
            case commands.reportAppState:
            case commands.heartbeat:
            case commands.adminExitStatus:
            case commands.theme:
            case commands.pcScreen:
            case commands.testSession:
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
    [names.commands, { view: view(), logo: null }, hub] as [
      typeof names.commands,
      unknown,
      string | null,
    ],
  );
  await page.goto("/");
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.hubStatus,
  );
  await expect(page.getByTestId("attract")).toBeVisible();
}

const queue = (page: Page, key: "__plans" | "__starts", ...answers: Answer[]) =>
  page.evaluate(
    ([k, a]) =>
      (window as unknown as Record<string, unknown[]>)[k as string].push(...(a as unknown[])),
    [key, answers] as const,
  );

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __calls: Array<{ cmd: string }> }).__calls);

const emit = (page: Page, event: string, payload: unknown) =>
  page.evaluate(
    ([e, p]) =>
      (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
        e as string,
        p,
      ),
    [event, payload],
  );

const picker = (page: Page) => page.getByTestId("plan-picker");
const cards = (page: Page) => page.getByTestId("picker-option");
const startButton = (page: Page) =>
  picker(page).getByRole("button", { name: en["player.picker.start"] });
const errorLine = (page: Page) => page.getByTestId("picker-error");

/** Logs in and lands on the picker with `first` as the options. */
async function toPicker(page: Page, first: Answer = FULL(), opts: Opts & { cards?: boolean } = {}) {
  await open(page, opts);
  await queue(page, "__plans", first);
  await page.keyboard.press("x");
  await expect(page.getByTestId("login-panel")).toBeVisible();
  await page.getByTestId("login-username").fill("asha");
  await page.getByTestId("login-password").fill("right-password");
  await page.getByTestId("login-password").press("Enter");
  await expect(picker(page)).toBeVisible();
  if (opts.cards !== false) await expect(cards(page).first()).toBeVisible();
}

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

test("the picker replaces the holding step with the cards in order and keeps Not you", async ({
  page,
}) => {
  await toPicker(page);
  await expect(page.getByTestId("login-holding")).toHaveCount(0);
  await expect(picker(page)).not.toContainText("Ask at the counter to start your time.");
  await expect(page.getByTestId("picker-player")).toHaveText("Logged in as Asha Rao");
  await expect(cards(page)).toHaveCount(5);
  expect(
    await cards(page).evaluateAll((els) => els.map((e) => e.getAttribute("data-mode"))),
  ).toEqual(["pack", "pack", "pass", "block", "wallet"]);
  await expect(picker(page).getByRole("button", { name: "Not you? Log out" })).toBeVisible();
  // Postpaid is not offered.
  await expect(picker(page)).not.toContainText(/postpaid|pay later|pay at/i);
  expect(await serious(page)).toEqual([]);
});

test("a pack shows its time left and expiry, a pass its window, a block its price and end", async ({
  page,
}) => {
  await toPicker(page);
  const [packCard, , passCard, blockCard, walletCardEl] = await cards(page).all();
  await expect(packCard).toContainText("10 hour pack");
  await expect(packCard).toContainText("2h 30m left");
  await expect(packCard).toContainText(/Expires .*2027/);
  await expect(passCard).toContainText("Night pass");
  await expect(passCard).toContainText("Plays 22:00–06:00");
  await expect(blockCard).toContainText("Night block");
  await expect(blockCard).toContainText("₹200.00");
  await expect(blockCard).toContainText("Ends at 06:00");
  // The wallet: balance, hourly rate and the rule chip.
  await expect(walletCardEl).toContainText("Wallet");
  await expect(walletCardEl).toContainText("Balance ₹40.00");
  await expect(walletCardEl).toContainText("₹120.00 an hour");
  const chip = page.getByTestId("picker-chip-rule");
  await expect(chip).toContainText("Happy hour −20%");
});

test("an expired pack and a closed pass are muted with their tag and cannot be chosen", async ({
  page,
}) => {
  await toPicker(
    page,
    plans([
      expiredPack(),
      pass({ selectable: false, tag: "outside_window" }),
      block({ selectable: false, tag: "outside_window", ends_label: undefined }),
      walletCard(),
    ]),
  );
  const [old, closedPass, closedBlock] = await cards(page).all();
  await expect(old).toContainText("Expired");
  await expect(old).toContainText("0h 45m unused");
  await expect(old).toBeDisabled();
  await expect(closedPass).toContainText("Outside hours");
  await expect(closedPass).toContainText("22:00–06:00");
  await expect(closedPass).toBeDisabled();
  await expect(closedBlock).toBeDisabled();
  await old.click({ force: true });
  await expect(old).toHaveAttribute("aria-checked", "false");
  await expect(startButton(page)).toHaveAttribute("aria-disabled", "true");
  expect(await serious(page)).toEqual([]);
});

test("Start waits for a choice; the chosen card gets a 2px text border", async ({ page }) => {
  await toPicker(page);
  const start = startButton(page);
  await expect(start).toHaveAttribute("aria-disabled", "true");
  await expect(picker(page)).toContainText(en["player.picker.start_disabled"]);
  const first = cards(page).first();
  await first.click();
  await expect(first).toHaveAttribute("aria-checked", "true");
  await expect(first).toHaveCSS("border-top-width", "2px");
  const text = await page.evaluate(() =>
    getComputedStyle(document.body).getPropertyValue("--text"),
  );
  expect(text).not.toBe("");
  const color = await first.evaluate((e) => getComputedStyle(e).borderTopColor);
  const probe = await page.evaluate((v) => {
    const el = document.createElement("span");
    el.style.color = v;
    document.body.append(el);
    const c = getComputedStyle(el).color;
    el.remove();
    return c;
  }, "var(--text)");
  expect(color).toBe(probe);
  await expect(start).not.toHaveAttribute("aria-disabled", "true");
  // Only one is chosen at a time.
  await cards(page).nth(3).click();
  await expect(first).toHaveAttribute("aria-checked", "false");
  await expect(cards(page).nth(3)).toHaveAttribute("aria-checked", "true");
});

test("the arrow keys move between the choosable cards, Enter and Space choose, Tab reaches Start", async ({
  page,
}) => {
  await toPicker(page);
  await expect(cards(page).nth(0)).toBeFocused();
  await page.keyboard.press("ArrowRight");
  // The expired pack is skipped.
  await expect(cards(page).nth(2)).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(cards(page).nth(3)).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(cards(page).nth(2)).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(cards(page).nth(2)).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("ArrowUp");
  await expect(cards(page).nth(0)).toBeFocused();
  await page.keyboard.press("Space");
  await expect(cards(page).nth(0)).toHaveAttribute("aria-checked", "true");
  await expect(cards(page).nth(2)).toHaveAttribute("aria-checked", "false");
  await page.keyboard.press("Tab");
  await expect(startButton(page)).toBeFocused();
  // Wraps around the end.
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("ArrowLeft");
  await expect(cards(page).nth(4)).toBeFocused();
});

test("Start sends the choice, shows Waiting for the counter with the plan, and disables everything", async ({
  page,
}) => {
  await toPicker(page);
  await queue(page, "__starts", OK);
  await cards(page).nth(2).click();
  await startButton(page).click();
  const waiting = page.getByTestId("picker-waiting");
  await expect(waiting).toBeVisible();
  await expect(waiting).toContainText("Waiting for the counter…");
  await expect(page.getByTestId("picker-waiting-plan")).toHaveText("Night pass");
  for (const card of await cards(page).all()) await expect(card).toBeDisabled();
  await expect(page.getByTestId("login-logout")).toBeDisabled();
  const sent = (await calls(page)).filter((c) => c.cmd === "start");
  expect(sent).toEqual([{ cmd: "start", args: { choice: { mode: "pass", pack_id: "S1" } } }]);
  expect(await serious(page)).toEqual([]);
});

test("each kind of card starts with its own mode and never names a player", async ({ page }) => {
  await toPicker(page);
  // No answer to Start puts the picker back each time, so every card can be tried.
  await queue(page, "__starts", null, null, null, null);
  for (const index of [0, 2, 3, 4]) {
    await cards(page).nth(index).click();
    await startButton(page).click();
    await expect(errorLine(page)).toContainText(en["player.hub.reconnecting_retry"]);
  }
  const sent = (await calls(page)).filter((c) => c.cmd === "start");
  expect(sent.map((c) => (c as unknown as { args: unknown }).args)).toEqual([
    { choice: { mode: "pack", pack_id: "K1" } },
    { choice: { mode: "pass", pack_id: "S1" } },
    { choice: { mode: "block", plan_id: "B1" } },
    { choice: { mode: "wallet" } },
  ]);
  expect(JSON.stringify(sent)).not.toMatch(/player_id|P1/);
});

test("an accepted start leaves for the session screen the moment pc-screen brings one", async ({
  page,
}) => {
  await toPicker(page);
  await queue(page, "__starts", OK);
  await cards(page).nth(0).click();
  await startButton(page).click();
  await expect(page.getByTestId("picker-waiting")).toBeVisible();
  await emit(page, "pc-screen", {
    locked: false,
    reason: null,
    session: {
      device_name: "PC 7",
      elapsed_ms: 500,
      warning_min: null,
      grace_in_ms: null,
      grace_ms: 0,
      ordered_items: 0,
    },
  });
  await expect(page.getByTestId("session")).toBeVisible();
  await expect(picker(page)).toHaveCount(0);
  await expect(page.getByTestId("attract")).toHaveCount(0);
});

test("no session screen within 10 s after ok brings the picker back with the reconnecting line", async ({
  page,
}) => {
  await toPicker(page, FULL(), { clock: true });
  await queue(page, "__starts", OK);
  await cards(page).nth(0).click();
  await startButton(page).click();
  await expect(page.getByTestId("picker-waiting")).toBeVisible();
  await page.clock.runFor(9_000);
  await expect(page.getByTestId("picker-waiting")).toBeVisible();
  await page.clock.runFor(2_000);
  await expect(page.getByTestId("picker-waiting")).toHaveCount(0);
  await expect(errorLine(page)).toContainText(en["player.hub.reconnecting_retry"]);
  await expect(cards(page).first()).toBeEnabled();
});

test("INSUFFICIENT_BALANCE keeps the player on the picker with the balance and refreshes the options", async ({
  page,
}) => {
  await toPicker(page, plans([pack(), walletCard()], 4000));
  await queue(page, "__starts", refused("INSUFFICIENT_BALANCE"));
  await queue(page, "__plans", plans([pack(), walletCard()], 4000));
  await cards(page).nth(1).click();
  await startButton(page).click();
  await expect(errorLine(page)).toContainText("Balance ₹40.00. Top up at counter.");
  await expect(errorLine(page)).toHaveAttribute("role", "alert");
  expect(en["player.balance.insufficient"]).toBe("Balance {balance}. Top up at counter.");
  // Selection cleared, picker open, options read again, another choice possible.
  await expect(picker(page)).toBeVisible();
  await expect(cards(page).nth(1)).toHaveAttribute("aria-checked", "false");
  await expect(startButton(page)).toHaveAttribute("aria-disabled", "true");
  expect((await calls(page)).filter((c) => c.cmd === "plans").length).toBe(2);
  await queue(page, "__starts", OK);
  await cards(page).nth(0).click();
  await expect(errorLine(page)).toHaveText("");
  await startButton(page).click();
  await expect(page.getByTestId("picker-waiting")).toBeVisible();
});

test("every other refusal reads from its code and leaves the picker open", async ({ page }) => {
  const cases: Array<[string, string]> = [
    ["PACK_EXPIRED", en["player.picker.refused.pack_expired"]],
    ["PLAN_NOT_FOR_GROUP", en["player.picker.refused.plan_not_for_group"]],
    ["PASS_OUTSIDE_WINDOW", en["player.picker.refused.pass_outside_window"]],
    ["PC_BUSY", en["player.picker.refused.pc_busy"]],
    ["NOT_FOUND", en["player.picker.refused.other"]],
  ];
  await toPicker(page, plans([pack(), walletCard()]));
  for (const [code, line] of cases) {
    await queue(page, "__starts", refused(code));
    await queue(page, "__plans", plans([pack(), walletCard()]));
    await cards(page).nth(0).click();
    await startButton(page).click();
    await expect(errorLine(page)).toContainText(line);
    await expect(picker(page)).toBeVisible();
    await expect(cards(page).nth(0)).toBeEnabled();
  }
});

test("with the Hub unreachable Start is not sent and the options stay", async ({ page }) => {
  await toPicker(page);
  await emit(page, "hub-status", { state: "reconnecting" });
  await cards(page).nth(0).click();
  const start = startButton(page);
  await expect(start).toHaveAttribute("aria-disabled", "true");
  await expect(picker(page)).toContainText(en["player.hub.reconnecting_retry"]);
  await start.click({ force: true });
  expect((await calls(page)).filter((c) => c.cmd === "start")).toEqual([]);
  await expect(cards(page)).toHaveCount(5);
  await emit(page, "hub-status", { state: "connected" });
  await expect(start).not.toHaveAttribute("aria-disabled", "true");
});

test("no answer to Start says the counter is reconnecting and lets the player try again", async ({
  page,
}) => {
  await toPicker(page);
  await queue(page, "__starts", null, OK);
  await cards(page).nth(0).click();
  await startButton(page).click();
  await expect(errorLine(page)).toContainText(en["player.hub.reconnecting_retry"]);
  await expect(page.getByTestId("picker-waiting")).toHaveCount(0);
  await expect(cards(page).nth(0)).toHaveAttribute("aria-checked", "true");
  await startButton(page).click();
  await expect(page.getByTestId("picker-waiting")).toBeVisible();
});

test("a Hub that lost the login sends the player back to attract", async ({ page }) => {
  await toPicker(page, FULL());
  await queue(page, "__starts", LOGIN_GONE);
  await cards(page).nth(0).click();
  await startButton(page).click();
  await expect(page.getByTestId("login-panel")).toHaveCount(0);
  await expect(page.getByTestId("attract")).toBeVisible();
});

test("Not you? forgets the player at the Hub and at the kiosk", async ({ page }) => {
  await toPicker(page);
  await page.getByTestId("login-logout").click();
  await expect(page.getByTestId("login-panel")).toHaveCount(0);
  await expect(page.getByTestId("attract")).toBeVisible();
  expect((await calls(page)).filter((c) => c.cmd === "logout").length).toBe(1);
  await page.keyboard.press("x");
  await expect(page.getByTestId("login-form")).toBeVisible();
  await expect(picker(page)).toHaveCount(0);
});

test("60 s without input on the picker forgets the player; input keeps it", async ({ page }) => {
  await toPicker(page, FULL(), { clock: true });
  await page.clock.runFor(59_000);
  await expect(picker(page)).toBeVisible();
  await page.keyboard.press("ArrowRight");
  await page.clock.runFor(59_000);
  await expect(picker(page)).toBeVisible();
  expect((await calls(page)).filter((c) => c.cmd === "logout")).toEqual([]);
  await page.clock.runFor(2_000);
  await expect(page.getByTestId("login-panel")).toHaveCount(0);
  await expect(page.getByTestId("attract")).toBeVisible();
  expect((await calls(page)).filter((c) => c.cmd === "logout").length).toBe(1);
});

test("a picker with nothing to offer says so, and nothing is kept on disk or in the DOM", async ({
  page,
}) => {
  await toPicker(page, { ok: true, body: { options: [] } } as Answer, { cards: false });
  await expect(page.getByTestId("picker-none")).toHaveText(en["player.picker.none"]);
  const traces = await page.evaluate(() => [
    JSON.stringify({ ...localStorage }),
    JSON.stringify({ ...sessionStorage }),
  ]);
  expect(traces.join("\n")).not.toMatch(/Asha|P1|right-password/);
  const filters = await page.evaluate(() =>
    [...document.querySelectorAll("*")]
      .map((el) => getComputedStyle(el).backdropFilter)
      .filter((f) => f && f !== "none"),
  );
  expect(filters).toEqual([]);
});

test("a picker whose options cannot be read says the counter is reconnecting", async ({ page }) => {
  await toPicker(page, null, { cards: false });
  await expect(picker(page)).toBeVisible();
  await expect(errorLine(page)).toContainText(en["player.hub.reconnecting_retry"]);
});

test("a lock over the PC forgets the held player, here and at the Hub", async ({ page }) => {
  await toPicker(page);
  await expect(picker(page)).toBeVisible();
  await emit(page, names.events.pcScreen, {
    locked: true,
    reason: "preparing_pc",
    session: null,
  });
  await expect(picker(page)).toHaveCount(0);
  await expect(page.getByTestId("login-panel")).toHaveCount(0);
  expect((await calls(page)).filter((c) => c.cmd === "logout")).toHaveLength(1);
});
