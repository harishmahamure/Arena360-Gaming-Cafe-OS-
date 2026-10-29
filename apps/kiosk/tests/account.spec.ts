import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import { accountPageOf, passwordChangeOf, validNewPassword } from "../src/lib/account";

// Story 6.34: the Account page (Details, Wallet & packs, Order history, Change password). The
// Tauri shell is stubbed in the page as in the account menu's spec: the test queues the answers
// the service would give (`__answers`, by command) and reads back what the page asked
// (`__calls`). What the player holds, the password check and the 60 s pause are the Hub's
// (Rust tests); the page shows what it is told.

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
            case commands.playerAccountPage:
            case commands.playerPasswordChange:
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

const PREPAID = { username: "asha", remaining_ms: 6_120_000, balance_minor: 25_050 };
const GUEST = { guest_no: 3 };

const shown = (session: Record<string, unknown> | null) => ({
  locked: false,
  reason: null,
  session: session && { device_name: "PC 7", elapsed_ms: 0, ...session },
});

const PAGE = {
  name: "Asha Rao",
  username: "asha",
  phone_masked: "+••••••••3210",
  balance_minor: 20_000,
  packs: [
    {
      name: "5 h",
      groups: ["PC", "VR"],
      minutes_left: 135,
      expires_at: Date.UTC(2026, 10, 20),
      expired: false,
    },
    {
      name: "Night pass",
      groups: ["PC"],
      window_min: [1320, 360],
      expires_at: Date.UTC(2026, 10, 25),
      expired: false,
    },
    {
      name: "10 h",
      groups: ["PC"],
      minutes_left: 45,
      expires_at: Date.UTC(2026, 9, 1),
      expired: true,
    },
  ],
  orders: [
    {
      order_id: "O2",
      at: Date.UTC(2026, 9, 28, 14, 30),
      items: [
        { name: "Cola", qty: 2 },
        { name: "Chips", qty: 1 },
      ],
      total_minor: 12_000,
      status: "settled",
      invoice_no: "MUM/26-27/E1/7",
    },
    {
      order_id: "O1",
      at: Date.UTC(2026, 9, 27, 9, 0),
      items: [{ name: "Burger", qty: 1 }],
      total_minor: 15_000,
      status: "open",
    },
  ],
};

async function signedIn(page: Page, s: Record<string, unknown> = PREPAID) {
  await emit(page, names.events.pcScreen, shown(s));
  await expect(page.getByTestId("shell")).toBeVisible();
}

async function openAccount(page: Page, body: unknown = PAGE) {
  await answer(page, names.commands.playerAccountPage, { ok: true, body });
  await page.getByTestId("account-chip").click();
  await page.getByTestId("account-row-account").click();
  await expect(page.getByTestId("account-page")).toBeVisible();
}

const nav = (page: Page, name: string) => page.getByTestId(`account-nav-${name}`);

test("the pure answers read as typed outcomes", () => {
  expect(accountPageOf(null)).toEqual({ kind: "unreachable" });
  expect(accountPageOf({ ok: true, body: { ...PAGE, packs: "x" } })).toEqual({
    kind: "unreachable",
  });
  expect(accountPageOf({ ok: true, body: PAGE })).toMatchObject({ kind: "ok" });
  const body = (outcome: string, more: object = {}) => ({ ok: true, body: { outcome, ...more } });
  expect(passwordChangeOf(body("ok"))).toEqual({ kind: "ok" });
  expect(passwordChangeOf(body("wrong_current"))).toEqual({ kind: "wrong_current" });
  expect(passwordChangeOf(body("invalid_new"))).toEqual({ kind: "invalid_new" });
  expect(passwordChangeOf(body("locked", { retry_s: 60 }))).toEqual({
    kind: "locked",
    retryS: 60,
  });
  expect(passwordChangeOf(body("refused", { message: "no" }))).toEqual({
    kind: "refused",
    message: "no",
  });
  expect(passwordChangeOf({ ok: false, body: null })).toEqual({ kind: "unreachable" });
  expect(validNewPassword("12345")).toBe(false);
  expect(validNewPassword("123456")).toBe(true);
  expect(validNewPassword("x".repeat(129))).toBe(false);
});

test("a guest has no Account row", async ({ page }) => {
  await open(page);
  await signedIn(page, GUEST);
  await page.getByTestId("account-chip").click();
  await expect(page.getByTestId("account-row-account")).toHaveCount(0);
});

test("Account opens below Messages with the sub-nav, and the tab the player came from stays active", async ({
  page,
}) => {
  await open(page);
  await signedIn(page);
  await page.getByTestId("tab-games").click();
  await page.getByTestId("account-chip").click();
  const rows = page.getByTestId("account-menu").getByRole("button");
  await expect(rows.nth(1)).toHaveText("Messages");
  await expect(rows.nth(2)).toHaveText("Account");
  await answer(page, names.commands.playerAccountPage, { ok: true, body: PAGE });
  await page.getByTestId("account-row-account").click();
  await expect(page.getByTestId("account-menu")).toHaveCount(0);
  await expect(page.getByTestId("account-subnav").getByRole("button")).toHaveText([
    "Details",
    "Wallet & packs",
    "Order history",
    "Change password",
  ]);
  await expect(nav(page, "details")).toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("tab-games")).toHaveAttribute("aria-current", "page");
  expect(await asked(page)).toEqual([names.commands.playerAccountPage]);
  expect(await serious(page)).toEqual([]);
  // A pill-nav tab closes the page.
  await page.getByTestId("tab-home").click();
  await expect(page.getByTestId("account-page")).toHaveCount(0);
});

test("the sub-nav takes arrow keys and marks the active item with a 3px left bar", async ({
  page,
}) => {
  await open(page);
  await signedIn(page);
  await openAccount(page);
  await nav(page, "details").focus();
  await page.keyboard.press("ArrowDown");
  await expect(nav(page, "wallet")).toBeFocused();
  await expect(nav(page, "wallet")).toHaveAttribute("aria-current", "page");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await expect(nav(page, "password")).toBeFocused();
  await expect(nav(page, "password")).toHaveCSS("border-left-width", "3px");
  await expect(nav(page, "details")).toHaveCSS("border-left-color", "rgba(0, 0, 0, 0)");
});

test("Details shows name, username and the phone masked but for its last 4 digits", async ({
  page,
}) => {
  await open(page);
  await signedIn(page);
  await openAccount(page);
  await expect(page.getByTestId("account-detail-name")).toHaveText("Asha Rao");
  await expect(page.getByTestId("account-detail-username")).toHaveText("asha");
  await expect(page.getByTestId("account-detail-phone")).toHaveText("+••••••••3210");
});

test("Wallet & packs shows the balance, the counter line, owned packs and expired ones muted", async ({
  page,
}) => {
  await open(page);
  await signedIn(page);
  await openAccount(page);
  await nav(page, "wallet").click();
  await expect(page.getByTestId("account-wallet")).toHaveText("Wallet ₹200.00");
  await expect(page.getByTestId("account-wallet-topups")).toHaveText(
    "Top-ups are accepted at the counter.",
  );
  const packs = page.getByTestId("account-pack");
  await expect(packs).toHaveCount(3);
  await expect(packs.nth(0)).toContainText("5 h");
  await expect(packs.nth(0)).toContainText("Valid on PC, VR");
  await expect(packs.nth(0)).toContainText("2h 15m left");
  await expect(packs.nth(0)).toContainText("Expires");
  await expect(packs.nth(1)).toContainText("Night pass");
  await expect(packs.nth(1)).toContainText("Daily 22:00–06:00");
  await expect(packs.nth(2)).toContainText("10 h");
  await expect(packs.nth(2)).toContainText("0h 45m unused");
  await expect(packs.nth(2).getByTestId("account-pack-expired")).toHaveText("Expired");
  await expect(packs.nth(2)).toHaveClass(/expired/);
  expect(await serious(page)).toEqual([]);
});

test("Order history lists orders newest first with date, items, total, status and invoice", async ({
  page,
}) => {
  await open(page);
  await signedIn(page);
  await openAccount(page);
  await nav(page, "orders").click();
  const orders = page.getByTestId("account-order");
  await expect(orders).toHaveCount(2);
  await expect(orders.nth(0)).toContainText("2× Cola, Chips");
  await expect(orders.nth(0)).toContainText("₹120.00");
  await expect(orders.nth(0).getByTestId("account-order-status")).toHaveText("Paid");
  await expect(orders.nth(0).getByTestId("account-order-invoice")).toHaveText(
    "Invoice MUM/26-27/E1/7",
  );
  await expect(orders.nth(1)).toContainText("Burger");
  await expect(orders.nth(1).getByTestId("account-order-status")).toHaveText("Not paid yet");
  await expect(orders.nth(1).getByTestId("account-order-invoice")).toHaveText("No invoice yet");
  expect(await serious(page)).toEqual([]);
});

test("an empty order history says so", async ({ page }) => {
  await open(page);
  await signedIn(page);
  await openAccount(page, { ...PAGE, orders: [], packs: [] });
  await nav(page, "orders").click();
  await expect(page.getByTestId("account-no-orders")).toHaveText("No orders yet.");
  await nav(page, "wallet").click();
  await expect(page.getByTestId("account-no-packs")).toBeVisible();
});

test("with the counter unreachable the page says so and asks for nothing", async ({ page }) => {
  await open(page, "reconnecting");
  await signedIn(page);
  await page.getByTestId("account-chip").click();
  await page.getByTestId("account-row-account").click();
  await expect(page.getByTestId("account-error")).toHaveText(
    "Counter is reconnecting — try again shortly.",
  );
  expect(await asked(page)).toEqual([]);
  await nav(page, "password").click();
  await expect(page.getByRole("button", { name: "Change password" }).last()).toBeDisabled();
});

test("Change password sends both passwords once, clears the fields and confirms", async ({
  page,
}) => {
  await open(page);
  await signedIn(page);
  await openAccount(page);
  await nav(page, "password").click();
  await page.getByTestId("account-password-current").fill("right-password");
  await page.getByTestId("account-password-new").fill("a-new-password");
  await answer(page, names.commands.playerPasswordChange, { ok: true, body: { outcome: "ok" } });
  await page.getByRole("button", { name: "Change password" }).last().click();
  await expect(page.getByTestId("account-password-saved")).toHaveText("✓ Password changed.");
  await expect(page.getByTestId("account-password-current")).toHaveValue("");
  await expect(page.getByTestId("account-password-new")).toHaveValue("");
  const sent = (await calls(page)).filter(([cmd]) => cmd === names.commands.playerPasswordChange);
  expect(sent).toEqual([
    [names.commands.playerPasswordChange, { current: "right-password", new: "a-new-password" }],
  ]);
  expect(await serious(page)).toEqual([]);
});

test("a short new password is refused in place; a wrong current one is a field error", async ({
  page,
}) => {
  await open(page);
  await signedIn(page);
  await openAccount(page);
  await nav(page, "password").click();
  await page.getByTestId("account-password-current").fill("right-password");
  await page.getByTestId("account-password-new").fill("short");
  await page.getByRole("button", { name: "Change password" }).last().click();
  await expect(page.getByTestId("account-password-error")).toHaveText(
    "⚠ Use at least 6 characters.",
  );
  await expect(page.getByTestId("account-password-new")).toBeFocused();
  expect((await asked(page)).includes(names.commands.playerPasswordChange)).toBe(false);
  await page.getByTestId("account-password-new").fill("a-new-password");
  await page.getByTestId("account-password-current").fill("wrong-one");
  await answer(page, names.commands.playerPasswordChange, {
    ok: true,
    body: { outcome: "wrong_current" },
  });
  await page.getByRole("button", { name: "Change password" }).last().click();
  await expect(page.getByTestId("account-password-error")).toHaveText(
    "⚠ That password isn't right.",
  );
  await expect(page.getByTestId("account-password-current")).toBeFocused();
  await expect(page.getByTestId("account-password-current")).toHaveAttribute(
    "aria-invalid",
    "true",
  );
  await expect(page.getByTestId("account-password-saved")).toHaveText("");
});

test("the fifth wrong current password pauses the form with a countdown", async ({ page }) => {
  await open(page);
  await signedIn(page);
  await openAccount(page);
  await nav(page, "password").click();
  await page.getByTestId("account-password-current").fill("wrong-one");
  await page.getByTestId("account-password-new").fill("a-new-password");
  await answer(page, names.commands.playerPasswordChange, {
    ok: true,
    body: { outcome: "locked", retry_s: 60 },
  });
  await page.getByRole("button", { name: "Change password" }).last().click();
  await expect(page.getByTestId("account-password-paused")).toContainText("Try again in 0:");
  await expect(page.getByTestId("account-password-current")).toBeDisabled();
  await expect(page.getByTestId("account-password-new")).toBeDisabled();
});
