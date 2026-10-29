import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { AttractView, PlayerAuthResult } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

// The player login (Story 6.8): the form in the attract screen's login panel, its outcomes, the
// lock countdown, the hand-over to the plan picker (Story 6.9) and the Hub link gating. The Tauri shell is stubbed in the
// page: `player_login` answers from a queue the test fills, and every call is recorded.

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
  locale: { ...INR },
  media: [],
  ...over,
});

type Answer = { ok: boolean; code?: string; body: unknown } | null;
const result = (r: PlayerAuthResult): Answer => ({ ok: true, body: r });
const ASHA = result({ outcome: "ok", player: { player_id: "P1", name: "Asha Rao" } });
const REJECTED = result({ outcome: "rejected" });
const LOCKED = result({ outcome: "locked", retry_s: 60 });

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
      w.__logins = [] as unknown[];
      w.__answers = [] as unknown[];
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event: string, id: number) => {
          listeners[event] = (listeners[event] ?? []).filter((x) => x !== id);
        },
      };
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
              const answers = w.__answers as unknown[];
              return answers.length > 0 ? answers.shift() : null;
            }
            case commands.playerPlans:
            case commands.playerLogout:
              return null;
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

const answer = (page: Page, ...answers: Answer[]) =>
  page.evaluate(
    (a) => (window as unknown as { __answers: unknown[] }).__answers.push(...a),
    answers,
  );

const logins = (page: Page) =>
  page.evaluate(() => (window as unknown as { __logins: unknown[] }).__logins);

const emit = (page: Page, event: string, payload: unknown) =>
  page.evaluate(
    ([e, p]) =>
      (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
        e as string,
        p,
      ),
    [event, payload],
  );

const panel = (page: Page) => page.getByTestId("login-panel");
const username = (page: Page) => page.getByTestId("login-username");
const password = (page: Page) => page.getByTestId("login-password");
const submit = (page: Page) => panel(page).getByRole("button", { name: en["player.login.submit"] });

async function openPanel(page: Page) {
  await page.keyboard.press("x");
  await expect(panel(page)).toBeVisible();
  await expect(username(page)).toBeFocused();
}

async function logIn(page: Page, user = "asha", pass = "right-password") {
  await username(page).fill(user);
  await password(page).fill(pass);
  await password(page).press("Enter");
}

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

test("the panel holds the form, the help links and the footer, and no other way in", async ({
  page,
}) => {
  await open(page);
  await openPanel(page);
  const p = panel(page);
  await expect(p.getByRole("heading", { name: en["player.attract.login.title"] })).toBeVisible();
  await expect(p.getByLabel(en["player.login.username"])).toBeVisible();
  await expect(p.getByLabel(en["player.login.password"])).toHaveAttribute("type", "password");
  await expect(submit(page)).toBeEnabled();
  await expect(p.getByRole("button", { name: en["player.login.forgot"] })).toBeVisible();
  await expect(p.getByRole("button", { name: en["player.login.create"] })).toBeVisible();
  const version = /\d+\.\d+\.\d+/;
  await expect(page.getByTestId("login-footer")).toHaveText(
    new RegExp(`^PC 7 · Main hall · v${version.source}$`),
  );
  // No language picker, social or QR login.
  await expect(
    p.locator("select, [role=combobox], [data-testid*=qr], [data-testid*=social]"),
  ).toHaveCount(0);
  await expect(p).not.toContainText(/google|facebook|apple|qr|language/i);
  expect(await serious(page)).toEqual([]);
});

test("the footer leaves out an empty zone", async ({ page }) => {
  await open(page);
  await emit(page, "attract", { view: view({ zone: "" }), logo: null });
  await openPanel(page);
  await expect(page.getByTestId("login-footer")).toHaveText(/^PC 7 · v\d+\.\d+\.\d+$/);
});

test("the password can be shown, hidden and pasted", async ({ page }) => {
  await open(page);
  await openPanel(page);
  const toggle = page.getByTestId("login-show");
  await expect(toggle).toHaveText(en["ui.password.show"]);
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await toggle.click();
  await expect(password(page)).toHaveAttribute("type", "text");
  await expect(toggle).toHaveText(en["ui.password.hide"]);
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  await toggle.click();
  await expect(password(page)).toHaveAttribute("type", "password");
  // Paste is not blocked.
  const pasted = await page.evaluate(() => {
    const input = document.querySelector<HTMLInputElement>("[data-testid=login-password]")!;
    input.focus();
    const data = new DataTransfer();
    data.setData("text/plain", "pasted-secret");
    const event = new ClipboardEvent("paste", { clipboardData: data, cancelable: true });
    input.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(pasted).toBe(false);
});

test("a correct login sends the credentials once and opens the plan picker", async ({ page }) => {
  await open(page);
  await openPanel(page);
  await answer(page, ASHA);
  await logIn(page, " asha ", "sample secret phrase");
  const holding = page.getByTestId("plan-picker");
  await expect(holding).toBeVisible();
  await expect(page.getByTestId("picker-player")).toHaveText("Logged in as Asha Rao");
  // The holding step is gone: the player picks their own time (Story 6.9).
  await expect(holding).not.toContainText("Ask at the counter to start your time.");
  await expect(holding.getByRole("button", { name: "Not you? Log out" })).toBeVisible();
  await expect(page.getByTestId("login-form")).toHaveCount(0);
  expect(await logins(page)).toEqual([{ username: "asha", password: "sample secret phrase" }]);
  // The password is nowhere on the page, in storage or in the saved state.
  const traces = await page.evaluate(() => [
    document.documentElement.outerHTML,
    JSON.stringify({ ...localStorage }),
    JSON.stringify({ ...sessionStorage }),
    ...[...document.querySelectorAll("input")].map((i) => i.value),
  ]);
  expect(traces.join("\n")).not.toContain("sample secret phrase");
  expect(await serious(page)).toEqual([]);
});

test("the password field is emptied the moment it is sent", async ({ page }) => {
  await open(page);
  await openPanel(page);
  await page.getByTestId("login-show").click();
  await answer(page, REJECTED);
  await username(page).fill("asha");
  await password(page).fill("wrong-one-9");
  await password(page).press("Enter");
  await expect(page.getByTestId("login-error")).toHaveText(/./);
  await expect(password(page)).toHaveValue("");
  await expect(password(page)).toHaveAttribute("type", "password");
  expect(await page.evaluate(() => document.documentElement.outerHTML)).not.toContain(
    "wrong-one-9",
  );
});

test("Enter and the Log in button both submit; an empty field just takes focus", async ({
  page,
}) => {
  await open(page);
  await openPanel(page);
  await submit(page).click();
  expect(await logins(page)).toEqual([]);
  await expect(username(page)).toBeFocused();
  await username(page).fill("asha");
  await submit(page).click();
  await expect(password(page)).toBeFocused();
  expect(await logins(page)).toEqual([]);
  await answer(page, REJECTED, ASHA);
  await password(page).fill("one");
  await submit(page).click();
  await expect(page.getByTestId("login-error")).toHaveText(/./);
  await password(page).fill("two");
  await submit(page).click();
  await expect(page.getByTestId("plan-picker")).toBeVisible();
  expect((await logins(page)).length).toBe(2);
});

test("a wrong username or password marks both fields, says one thing and refocuses", async ({
  page,
}) => {
  await open(page);
  await openPanel(page);
  await answer(page, REJECTED);
  await username(page).fill("asha");
  await password(page).fill("nope-nope");
  await password(page).press("Enter");
  const error = page.getByTestId("login-error");
  await expect(error).toContainText("Username or password didn't match.");
  await expect(error).toContainText(en["player.login.failure"]);
  await expect(error).toHaveAttribute("role", "alert");
  for (const field of [username(page), password(page)]) {
    await expect(field).toHaveAttribute("aria-invalid", "true");
    await expect(field).toHaveCSS("border-top-width", "2px");
  }
  await expect(username(page)).toBeFocused();
  await expect(password(page)).toHaveValue("");
  // It never says which field was wrong.
  await expect(error).not.toContainText(/username is|password is|unknown|no such/i);
  expect(await serious(page)).toEqual([]);
  // The next try clears the treatment.
  await answer(page, ASHA);
  await password(page).fill("right");
  await password(page).press("Enter");
  await expect(page.getByTestId("plan-picker")).toBeVisible();
});

test("five wrong tries lock the form with a counting-down message, then it opens again", async ({
  page,
}) => {
  await open(page, { clock: true });
  await openPanel(page);
  await answer(page, REJECTED, REJECTED, REJECTED, REJECTED, LOCKED);
  for (let i = 0; i < 5; i++) {
    await username(page).fill("asha");
    await password(page).fill(`nope-${i}`);
    await password(page).press("Enter");
    if (i < 4) await expect(page.getByTestId("login-error")).toContainText("didn't match");
  }
  const locked = "Too many tries. Try again in 0:60.";
  await expect(panel(page)).toContainText(locked);
  expect(en["player.login.locked"]).toBe("Too many tries. Try again in {countdown}.");
  await expect(username(page)).toBeDisabled();
  await expect(password(page)).toBeDisabled();
  await expect(submit(page)).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByTestId("login-error")).toHaveText("");
  // Tabular figures, counting down.
  await expect(page.getByTestId("login-locked").locator("span.reason")).toHaveCSS(
    "font-variant-numeric",
    /tabular-nums/,
  );
  await page.clock.runFor(1_000);
  await expect(panel(page)).toContainText("Too many tries. Try again in 0:59.");
  await page.clock.runFor(48_000);
  await expect(panel(page)).toContainText("Try again in 0:11.");
  // A locked form sends nothing, whatever is pressed.
  await page.keyboard.press("Enter");
  expect((await logins(page)).length).toBe(5);
  await page.clock.runFor(11_500);
  await expect(username(page)).toBeEnabled();
  await expect(username(page)).toBeFocused();
  await expect(submit(page)).toBeEnabled();
  await expect(panel(page)).not.toContainText("Too many tries");
  // The count started again: the next wrong try is just a wrong try.
  await answer(page, REJECTED);
  await username(page).fill("asha");
  await password(page).fill("again");
  await password(page).press("Enter");
  await expect(page.getByTestId("login-error")).toContainText("didn't match");
  await expect(username(page)).toBeEnabled();
});

test("five wrong answers in a row lock the screen even if the Hub did not say so", async ({
  page,
}) => {
  await open(page);
  await openPanel(page);
  await answer(page, REJECTED, REJECTED, REJECTED, REJECTED, REJECTED);
  for (let i = 0; i < 5; i++) {
    await username(page).fill("asha");
    await password(page).fill(`nope-${i}`);
    await password(page).press("Enter");
    if (i < 4) await expect(page.getByTestId("login-error")).toContainText("didn't match");
  }
  await expect(panel(page)).toContainText(/Too many tries\. Try again in 0:(60|59)\./);
  await expect(username(page)).toBeDisabled();
});

test("a correct login clears the count of wrong tries", async ({ page }) => {
  await open(page);
  await openPanel(page);
  await answer(page, REJECTED, REJECTED, REJECTED, REJECTED, ASHA);
  for (let i = 0; i < 5; i++) {
    await username(page).fill("asha");
    await password(page).fill(i < 4 ? `nope-${i}` : "right");
    await password(page).press("Enter");
    if (i < 4) await expect(page.getByTestId("login-error")).toContainText("didn't match");
  }
  await expect(page.getByTestId("plan-picker")).toBeVisible();
  await page.getByTestId("login-logout").click();
  await page.keyboard.press("x");
  await expect(username(page)).toBeEnabled();
  await answer(page, REJECTED);
  await logIn(page, "asha", "nope");
  await expect(page.getByTestId("login-error")).toContainText("didn't match");
  await expect(username(page)).toBeEnabled();
});

test("a player already playing elsewhere is told which PC", async ({ page }) => {
  await open(page);
  await openPanel(page);
  await answer(page, result({ outcome: "busy", device_name: "PC 3" }));
  await logIn(page);
  const error = page.getByTestId("login-error");
  await expect(error).toContainText("You're already playing on PC 3. Ask at the counter.");
  await expect(page.getByTestId("plan-picker")).toHaveCount(0);
  await expect(password(page)).toHaveValue("");
  await expect(username(page)).toBeFocused();
  // Not a wrong try: the form stays open for more.
  await expect(username(page)).toBeEnabled();
});

test("Forgot password and Create account answer in place", async ({ page }) => {
  await open(page);
  await openPanel(page);
  const note = page.getByTestId("login-note");
  await expect(note).toHaveText("");
  await page.getByTestId("login-forgot").click();
  await expect(note).toHaveText("Ask staff at the counter to reset it.");
  await expect(panel(page)).toBeVisible();
  await page.getByTestId("login-create").click();
  await expect(note).toHaveText("Ask at the counter to sign up.");
  await expect(panel(page)).toBeVisible();
  expect((await logins(page)).length).toBe(0);
  // Underlined, like links.
  await expect(page.getByTestId("login-forgot")).toHaveCSS("text-decoration-line", "underline");
  await expect(page.getByTestId("login-create")).toHaveCSS("text-decoration-line", "underline");
});

test("with the Hub unreachable Log in is disabled with its reason and wakes on reconnect", async ({
  page,
}) => {
  await open(page, { hub: "reconnecting" });
  await openPanel(page);
  const reason = "Counter is reconnecting — try again shortly.";
  await expect(panel(page)).toContainText(reason);
  expect(en["player.hub.reconnecting_retry"]).toBe(reason);
  await expect(submit(page)).toHaveAttribute("aria-disabled", "true");
  await username(page).fill("asha");
  await password(page).fill("whatever");
  await password(page).press("Enter");
  await submit(page).click({ force: true });
  expect(await logins(page)).toEqual([]);
  expect(await serious(page)).toEqual([]);
  await emit(page, "hub-status", { state: "connected" });
  await expect(panel(page)).not.toContainText(reason);
  await expect(submit(page)).toBeEnabled();
  await answer(page, ASHA);
  await submit(page).click();
  await expect(page.getByTestId("plan-picker")).toBeVisible();
  // And away again: the link going down while the form shows disables it at once.
  await page.getByTestId("login-logout").click();
  await page.keyboard.press("x");
  await emit(page, "hub-status", { state: "reconnecting" });
  await expect(submit(page)).toHaveAttribute("aria-disabled", "true");
});

test("a link not yet known counts as unreachable", async ({ page }) => {
  await open(page, { hub: null });
  await openPanel(page);
  await expect(submit(page)).toHaveAttribute("aria-disabled", "true");
  await emit(page, "hub-status", { state: "connected" });
  await expect(submit(page)).toBeEnabled();
});

test("no answer in time says the counter is reconnecting and lets the player try again", async ({
  page,
}) => {
  await open(page);
  await openPanel(page);
  await answer(page, null, ASHA);
  await logIn(page);
  await expect(page.getByTestId("login-error")).toContainText(en["player.hub.reconnecting_retry"]);
  await expect(username(page)).toHaveValue("asha");
  await expect(submit(page)).toBeEnabled();
  await password(page).fill("again");
  await password(page).press("Enter");
  await expect(page.getByTestId("plan-picker")).toBeVisible();
});

test("the plan picker goes back to attract on Not you, and on 60 s without input", async ({
  page,
}) => {
  await open(page, { clock: true });
  await openPanel(page);
  await answer(page, ASHA, ASHA);
  await logIn(page);
  const holding = page.getByTestId("plan-picker");
  await expect(holding).toBeVisible();
  // Esc closes the form, not the holding step.
  await page.keyboard.press("Escape");
  await expect(holding).toBeVisible();
  await page.getByTestId("login-logout").click();
  await expect(panel(page)).toHaveCount(0);
  await expect(page.getByTestId("attract")).toBeVisible();
  // The player is forgotten: the next panel is a fresh form.
  await page.keyboard.press("x");
  await expect(page.getByTestId("login-form")).toBeVisible();
  await expect(username(page)).toHaveValue("");
  await logIn(page);
  await expect(holding).toBeVisible();
  await page.clock.runFor(59_000);
  await expect(holding).toBeVisible();
  // Input keeps it.
  await page.keyboard.press("Shift");
  await page.clock.runFor(59_000);
  await expect(holding).toBeVisible();
  await page.clock.runFor(2_000);
  await expect(panel(page)).toHaveCount(0);
  await expect(page.getByTestId("attract")).toBeVisible();
  await page.keyboard.press("x");
  await expect(page.getByTestId("login-form")).toBeVisible();
  await expect(page.getByTestId("plan-picker")).toHaveCount(0);
});

test("a session starting leaves the plan picker for the session screen, forgetting the player", async ({
  page,
}) => {
  await open(page);
  await openPanel(page);
  await answer(page, ASHA);
  await logIn(page);
  await expect(page.getByTestId("plan-picker")).toBeVisible();
  await emit(page, "pc-screen", {
    locked: false,
    reason: null,
    message: null,
    session: {
      device_name: "PC 7",
      elapsed_ms: 1000,
      warning_min: null,
      grace_in_ms: null,
      grace_ms: 0,
      ordered_items: 0,
    },
  });
  await expect(page.getByTestId("session")).toBeVisible();
  await expect(page.getByTestId("attract")).toHaveCount(0);
  await expect(page.getByTestId("plan-picker")).toHaveCount(0);
  // Back to idle: the player was forgotten with the screen.
  await emit(page, "pc-screen", { locked: false, reason: null, message: null, session: null });
  await expect(page.getByTestId("attract")).toBeVisible();
  await page.keyboard.press("x");
  await expect(page.getByTestId("login-form")).toBeVisible();
  await expect(page.getByTestId("plan-picker")).toHaveCount(0);
});

test("the form is keyboard-only friendly and uses no blur", async ({ page }) => {
  await open(page);
  await openPanel(page);
  // Tab order: username, password, show, Log in, Close, Forgot, Create.
  const order = [];
  for (let i = 0; i < 7; i++) {
    order.push(
      await page.evaluate(
        () =>
          document.activeElement?.getAttribute("data-testid") ??
          document.activeElement?.textContent?.trim(),
      ),
    );
    await page.keyboard.press("Tab");
  }
  expect(order.slice(0, 3)).toEqual(["login-username", "login-password", "login-show"]);
  const filters = await page.evaluate(() =>
    [...document.querySelectorAll("*")]
      .map((el) => getComputedStyle(el).backdropFilter)
      .filter((f) => f && f !== "none"),
  );
  expect(filters).toEqual([]);
});
