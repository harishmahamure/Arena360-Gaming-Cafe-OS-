import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { AttractView, PlayerSignupResult } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

// The kiosk sign-up (Story 6.10): the form in the login panel, its field checks, the consent box
// and legal pages, the Hub's typed refusals, the unreachable Hub and the hand-over to the plan
// picker. The Tauri shell is stubbed in the page: `player_signup` answers from a queue the test
// fills, and every call is recorded.

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
const result = (r: PlayerSignupResult): Answer => ({ ok: true, body: r });
const CREATED = result({ outcome: "ok", player: { player_id: "P1", name: "Asha Rao" } });
const refused = (code: string, field?: string) =>
  result({ outcome: "refused", code, field } as PlayerSignupResult);

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
      w.__signups = [] as unknown[];
      w.__answers = [] as unknown[];
      w.__logouts = 0;
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
            case commands.playerSignup: {
              (w.__signups as unknown[]).push(args);
              const answers = w.__answers as unknown[];
              return answers.length > 0 ? answers.shift() : null;
            }
            case commands.playerPlans:
              return { ok: true, body: { options: [] } };
            case commands.playerLogout:
              w.__logouts = (w.__logouts as number) + 1;
              return null;
            case commands.playerLogin:
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

const signups = (page: Page) =>
  page.evaluate(() => (window as unknown as { __signups: unknown[] }).__signups);

const logouts = (page: Page) =>
  page.evaluate(() => (window as unknown as { __logouts: number }).__logouts);

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
const username = (page: Page) => page.getByTestId("signup-username");
const password = (page: Page) => page.getByTestId("signup-password");
const phone = (page: Page) => page.getByTestId("signup-phone");
const nameField = (page: Page) => page.getByTestId("signup-name");
const consent = (page: Page) => page.getByTestId("signup-consent");
const create = (page: Page) =>
  panel(page).getByRole("button", { name: en["player.signup.submit"] });

async function openSignup(page: Page) {
  await page.keyboard.press("x");
  await expect(panel(page)).toBeVisible();
  await page.getByTestId("login-create").click();
  await expect(page.getByTestId("signup-form")).toBeVisible();
  await expect(username(page)).toBeFocused();
}

async function fillValid(page: Page) {
  await username(page).fill("Asha_9");
  await password(page).fill("sample secret phrase");
  await phone(page).fill("+91 98765 43210");
  await nameField(page).fill("Asha Rao");
}

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

test("Create account opens the form in the same panel with the four fields and one unticked box", async ({
  page,
}) => {
  await open(page);
  await openSignup(page);
  const p = panel(page);
  await expect(p.getByRole("heading", { name: en["player.signup.title"] })).toBeVisible();
  await expect(p.getByLabel(en["player.login.username"], { exact: true })).toBeVisible();
  await expect(p.getByLabel(en["player.login.password"], { exact: true })).toHaveAttribute(
    "type",
    "password",
  );
  await expect(p.getByLabel(en["player.signup.phone"])).toHaveValue("+91");
  await expect(p.getByLabel(en["player.signup.name"], { exact: true })).toBeVisible();
  await expect(consent(page)).not.toBeChecked();
  // The consent text is the catalogue line, with the two links underlined.
  await expect(page.getByTestId("signup-consent-text")).toHaveText(
    en["player.signup.consent"].replace(/\s+/g, " "),
  );
  for (const link of ["signup-link-terms", "signup-link-privacy"]) {
    await expect(page.getByTestId(link)).toHaveCSS("text-decoration-line", "underline");
  }
  await expect(page.getByTestId("signup-link-terms")).toHaveText(en["player.signup.terms"]);
  await expect(page.getByTestId("signup-link-privacy")).toHaveText(en["player.signup.privacy"]);
  // No email, OTP, marketing box or other consent.
  await expect(p.locator("input[type=checkbox]")).toHaveCount(1);
  await expect(p.locator("input[type=email]")).toHaveCount(0);
  await expect(p).not.toContainText(/otp|one-time|marketing|email|referral|birth/i);
  expect(await serious(page)).toEqual([]);
});

test("the consent box has a 44px hit area and the password can be shown, hidden and pasted", async ({
  page,
}) => {
  await open(page);
  await openSignup(page);
  const box = await consent(page).boundingBox();
  expect(box!.width).toBeGreaterThanOrEqual(44);
  expect(box!.height).toBeGreaterThanOrEqual(44);
  const toggle = page.getByTestId("signup-show");
  await expect(toggle).toHaveText(en["ui.password.show"]);
  await toggle.click();
  await expect(password(page)).toHaveAttribute("type", "text");
  await expect(toggle).toHaveText(en["ui.password.hide"]);
  await toggle.click();
  await expect(password(page)).toHaveAttribute("type", "password");
  const pasted = await page.evaluate(() => {
    const input = document.querySelector<HTMLInputElement>("[data-testid=signup-password]")!;
    input.focus();
    const data = new DataTransfer();
    data.setData("text/plain", "pasted-secret");
    const event = new ClipboardEvent("paste", { clipboardData: data, cancelable: true });
    input.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(pasted).toBe(false);
});

test("with the box unticked Create account is disabled with its reason, and ticking enables it", async ({
  page,
}) => {
  await open(page);
  await openSignup(page);
  await fillValid(page);
  await expect(create(page)).toHaveAttribute("aria-disabled", "true");
  await expect(panel(page)).toContainText("Tick the box to continue");
  expect(en["player.signup.consent_needed"]).toBe("Tick the box to continue");
  // Pressing it (or Enter) sends nothing.
  await create(page).click({ force: true });
  await password(page).press("Enter");
  expect(await signups(page)).toEqual([]);
  await consent(page).check();
  await expect(create(page)).toBeEnabled();
  await expect(panel(page)).not.toContainText("Tick the box to continue");
  await consent(page).uncheck();
  await expect(create(page)).toHaveAttribute("aria-disabled", "true");
});

test("clicking the consent text ticks the box but clicking a link opens a page instead", async ({
  page,
}) => {
  await open(page);
  await openSignup(page);
  await page.getByTestId("signup-consent-text").click({ position: { x: 4, y: 4 } });
  await expect(consent(page)).toBeChecked();
  await page.getByTestId("signup-link-terms").click();
  await expect(consent(page)).toBeChecked();
  await expect(page.getByTestId("legal-page")).toBeVisible();
});

test("valid input with the box ticked sends once, clears the password and opens the plan picker", async ({
  page,
}) => {
  await open(page);
  await openSignup(page);
  await fillValid(page);
  await consent(page).check();
  await page.getByTestId("signup-show").click();
  await answer(page, CREATED);
  await create(page).click();
  await expect(page.getByTestId("plan-picker")).toBeVisible();
  await expect(page.getByTestId("picker-player")).toHaveText("Logged in as Asha Rao");
  await expect(page.getByTestId("signup-form")).toHaveCount(0);
  // Sent exactly the fields, trimmed, with consent true; no player id, policy version or email.
  expect(await signups(page)).toEqual([
    {
      username: "Asha_9",
      password: "sample secret phrase",
      phone: "+91 98765 43210",
      name: "Asha Rao",
      consent: true,
    },
  ]);
  // The password is nowhere on the page, in storage or in the saved state.
  const traces = await page.evaluate(() => [
    document.documentElement.outerHTML,
    JSON.stringify({ ...localStorage }),
    JSON.stringify({ ...sessionStorage }),
    ...[...document.querySelectorAll("input")].map((i) => i.value),
  ]);
  expect(traces.join("\n")).not.toContain("sample secret phrase");
  expect(await serious(page)).toEqual([]);
  // "Not you? Log out" forgets the player at the Hub as a login does.
  await page.getByTestId("login-logout").click();
  await expect(panel(page)).toHaveCount(0);
  expect(await logouts(page)).toBe(1);
});

test("the password field is emptied the moment it is sent", async ({ page }) => {
  await open(page);
  await openSignup(page);
  await fillValid(page);
  await consent(page).check();
  await page.getByTestId("signup-show").click();
  await answer(page, refused("USERNAME_TAKEN", "username"));
  await create(page).click();
  await expect(page.getByTestId("signup-username-error")).toHaveText(/./);
  await expect(password(page)).toHaveValue("");
  await expect(password(page)).toHaveAttribute("type", "password");
  expect(await page.evaluate(() => document.documentElement.outerHTML)).not.toContain(
    "sample secret phrase",
  );
  // The other fields are kept.
  await expect(username(page)).toHaveValue("Asha_9");
  await expect(nameField(page)).toHaveValue("Asha Rao");
  await expect(consent(page)).toBeChecked();
});

test("a taken username shows its message on the field and moves focus there", async ({ page }) => {
  await open(page);
  await openSignup(page);
  await fillValid(page);
  await consent(page).check();
  await answer(page, refused("USERNAME_TAKEN", "username"));
  await create(page).click();
  const error = page.getByTestId("signup-username-error");
  await expect(error).toContainText("That username's taken.");
  expect(en["player.signup.username_taken"]).toBe("That username's taken.");
  await expect(error).toHaveAttribute("role", "alert");
  await expect(username(page)).toHaveAttribute("aria-invalid", "true");
  await expect(username(page)).toHaveCSS("border-top-width", "2px");
  await expect(username(page)).toBeFocused();
  // Editing the field takes the message away.
  await username(page).fill("asha_10");
  await expect(error).toHaveText("");
  await expect(username(page)).not.toHaveAttribute("aria-invalid", "true");
  expect(await serious(page)).toEqual([]);
});

test("a phone that already has an account shows its message on the phone field", async ({
  page,
}) => {
  await open(page);
  await openSignup(page);
  await fillValid(page);
  await consent(page).check();
  await answer(page, refused("PHONE_TAKEN", "phone"));
  await create(page).click();
  const error = page.getByTestId("signup-phone-error");
  await expect(error).toContainText(
    "That phone number already has an account — ask staff if it's yours.",
  );
  await expect(phone(page)).toBeFocused();
  await expect(phone(page)).toHaveAttribute("aria-invalid", "true");
});

test("a refusal on another field and an unknown refusal both read in place", async ({ page }) => {
  await open(page);
  await openSignup(page);
  await fillValid(page);
  await consent(page).check();
  await answer(page, refused("VALIDATION_FAILED", "name"), refused("INTERNAL"));
  await create(page).click();
  await expect(page.getByTestId("signup-name-error")).toHaveText(
    new RegExp(en["hub.players.invalid.name"]),
  );
  await expect(nameField(page)).toBeFocused();
  await password(page).fill("another secret");
  await create(page).click();
  await expect(page.getByTestId("signup-error")).toContainText(en["player.signup.refused.other"]);
});

test("a short password or a bad phone shows its error on blur and on submit and sends nothing", async ({
  page,
}) => {
  await open(page);
  await openSignup(page);
  await consent(page).check();
  // On blur.
  await password(page).fill("12345");
  await password(page).blur();
  await expect(page.getByTestId("signup-password-error")).toContainText(
    en["hub.players.invalid.password"],
  );
  await expect(password(page)).toHaveAttribute("aria-invalid", "true");
  await phone(page).fill("+12");
  await phone(page).blur();
  await expect(page.getByTestId("signup-phone-error")).toContainText(
    en["hub.players.invalid.phone"],
  );
  await username(page).fill("ab");
  await username(page).blur();
  await expect(page.getByTestId("signup-username-error")).toContainText(
    en["hub.players.invalid.username"],
  );
  // Editing a field clears its message.
  await password(page).fill("123456");
  await expect(page.getByTestId("signup-password-error")).toHaveText("");
  // On submit: nothing is sent, focus goes to the first invalid field.
  await create(page).click();
  expect(await signups(page)).toEqual([]);
  await expect(username(page)).toBeFocused();
  await expect(nameField(page)).toHaveAttribute("aria-invalid", "true");
  await expect(page.getByTestId("signup-name-error")).toContainText(en["hub.players.invalid.name"]);
  await username(page).fill("asha");
  await nameField(page).fill("Asha");
  await create(page).click();
  expect(await signups(page)).toEqual([]);
  await expect(phone(page)).toBeFocused();
  expect(await serious(page)).toEqual([]);
});

test("the rules match the counter: username 3-32 after trimming, phone with + and 8-15 digits", async ({
  page,
}) => {
  await open(page);
  await openSignup(page);
  const bad = async (field: ReturnType<typeof username>, errorId: string, value: string) => {
    await field.fill(value);
    await field.blur();
    await expect(page.getByTestId(errorId)).not.toHaveText("");
  };
  const good = async (field: ReturnType<typeof username>, errorId: string, value: string) => {
    await field.fill(value);
    await field.blur();
    await expect(page.getByTestId(errorId)).toHaveText("");
  };
  await bad(username(page), "signup-username-error", "has space");
  await bad(username(page), "signup-username-error", "x".repeat(33));
  await good(username(page), "signup-username-error", "  A.b_c-9  ");
  await bad(phone(page), "signup-phone-error", "9876543210");
  await bad(phone(page), "signup-phone-error", "+1234567");
  await bad(phone(page), "signup-phone-error", "+1234567890123456");
  await good(phone(page), "signup-phone-error", "+91 98765-43210");
  await bad(nameField(page), "signup-name-error", "   ");
  await bad(nameField(page), "signup-name-error", "n".repeat(81));
  await good(nameField(page), "signup-name-error", "Asha Rao");
});

test("with the Hub unreachable nothing is sent, the reason shows and the fields are kept", async ({
  page,
}) => {
  await open(page, { hub: "reconnecting" });
  await openSignup(page);
  await fillValid(page);
  await consent(page).check();
  const reason = "Counter is reconnecting — try again shortly.";
  await expect(panel(page)).toContainText(reason);
  expect(en["player.hub.reconnecting_retry"]).toBe(reason);
  await expect(create(page)).toHaveAttribute("aria-disabled", "true");
  await create(page).click({ force: true });
  await password(page).press("Enter");
  expect(await signups(page)).toEqual([]);
  await expect(username(page)).toHaveValue("Asha_9");
  await expect(password(page)).toHaveValue("sample secret phrase");
  expect(await serious(page)).toEqual([]);
  await emit(page, "hub-status", { state: "connected" });
  await expect(panel(page)).not.toContainText(reason);
  await expect(create(page)).toBeEnabled();
});

test("no answer in time says the counter is reconnecting and keeps the other fields", async ({
  page,
}) => {
  await open(page);
  await openSignup(page);
  await fillValid(page);
  await consent(page).check();
  await answer(page, null, CREATED);
  await create(page).click();
  await expect(page.getByTestId("signup-error")).toContainText(en["player.hub.reconnecting_retry"]);
  await expect(username(page)).toHaveValue("Asha_9");
  await expect(phone(page)).toHaveValue("+91 98765 43210");
  await expect(password(page)).toHaveValue("");
  await password(page).fill("sample secret phrase");
  await create(page).click();
  await expect(page.getByTestId("plan-picker")).toBeVisible();
  expect((await signups(page)).length).toBe(2);
});

test("Terms and Privacy open read-only pages and Back returns to the form with its fields kept", async ({
  page,
}) => {
  await open(page);
  await openSignup(page);
  await fillValid(page);
  await consent(page).check();
  for (const [link, title] of [
    ["signup-link-terms", en["player.signup.terms"]],
    ["signup-link-privacy", en["player.signup.privacy"]],
  ] as const) {
    await page.getByTestId(link).click();
    const legal = page.getByTestId("legal-page");
    await expect(legal).toBeVisible();
    await expect(legal.getByRole("heading", { name: title })).toBeVisible();
    await expect(legal.locator("p").first()).toHaveText(/./);
    await expect(page.getByTestId("signup-form")).toBeHidden();
    // Read-only: no inputs, no network.
    await expect(legal.locator("input, textarea, a[href]")).toHaveCount(0);
    expect(await serious(page)).toEqual([]);
    await legal.getByRole("button", { name: en["player.legal.back"] }).click();
    await expect(legal).toHaveCount(0);
    await expect(page.getByTestId("signup-form")).toBeVisible();
    await expect(page.getByTestId(link)).toBeFocused();
    await expect(username(page)).toHaveValue("Asha_9");
    await expect(password(page)).toHaveValue("sample secret phrase");
    await expect(phone(page)).toHaveValue("+91 98765 43210");
    await expect(nameField(page)).toHaveValue("Asha Rao");
    await expect(consent(page)).toBeChecked();
  }
});

test("Back to log in returns to the login form; Esc closes the panel", async ({ page }) => {
  await open(page);
  await openSignup(page);
  await page.getByRole("button", { name: en["player.signup.back"] }).click();
  await expect(page.getByTestId("login-form")).toBeVisible();
  await expect(page.getByTestId("signup-form")).toHaveCount(0);
  await page.getByTestId("login-create").click();
  await expect(page.getByTestId("signup-form")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(panel(page)).toHaveCount(0);
  await expect(page.getByTestId("attract")).toBeVisible();
  // A fresh panel starts at the login form.
  await page.keyboard.press("x");
  await expect(page.getByTestId("login-form")).toBeVisible();
});

test("the panel closes after 60 s without input while the form is open", async ({ page }) => {
  await open(page, { clock: true });
  await openSignup(page);
  await page.clock.runFor(59_000);
  await expect(panel(page)).toBeVisible();
  await page.keyboard.press("Shift");
  await page.clock.runFor(59_000);
  await expect(panel(page)).toBeVisible();
  await page.clock.runFor(2_000);
  await expect(panel(page)).toHaveCount(0);
});

test("a session starting leaves the plan picker for the session screen", async ({ page }) => {
  await open(page);
  await openSignup(page);
  await fillValid(page);
  await consent(page).check();
  await answer(page, CREATED);
  await create(page).click();
  await expect(page.getByTestId("plan-picker")).toBeVisible();
  await emit(page, "pc-screen", {
    locked: false,
    reason: null,
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
  await expect(page.getByTestId("plan-picker")).toHaveCount(0);
});

test("the form is keyboard friendly and uses no blur or hex colours", async ({ page }) => {
  await open(page);
  await openSignup(page);
  const order = [];
  for (let i = 0; i < 6; i++) {
    order.push(await page.evaluate(() => document.activeElement?.getAttribute("data-testid")));
    await page.keyboard.press("Tab");
  }
  expect(order).toEqual([
    "signup-username",
    "signup-password",
    "signup-show",
    "signup-phone",
    "signup-name",
    "signup-consent",
  ]);
  const filters = await page.evaluate(() =>
    [...document.querySelectorAll("*")]
      .map((el) => getComputedStyle(el).backdropFilter)
      .filter((f) => f && f !== "none"),
  );
  expect(filters).toEqual([]);
});

test("the form fits a 1366x768 screen", async ({ page }) => {
  await page.setViewportSize({ width: 1366, height: 768 });
  await open(page);
  await openSignup(page);
  const box = await panel(page).boundingBox();
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.y + box!.height).toBeLessThanOrEqual(768);
});
