import { expect, test, type Page } from "@playwright/test";
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import { ATTRACT, PREPAID, emit, openKiosk, shown } from "./support";

// Story 6.37: the player journey by keyboard alone, from the login to Log out. Tab and Shift+Tab
// reach every control, Enter and Space act, arrow keys step through the pill nav, the sub-nav and
// the grids, and Esc backs out. The focus ring is visible at every stop. No mouse is used: the
// only `click` in this file would be a bug. The Ctrl+Shift+E hotkey is the Rust keyboard hook
// (Story 6.14), not the page, so it is covered by the Rust hud tests.

const focused = (page: Page) =>
  page.evaluate(() => {
    const el = document.activeElement;
    return (
      el?.getAttribute("data-testid") ??
      (el ? `<${el.tagName.toLowerCase()} .${el.className}>` : null)
    );
  });

/** The focus ring on the focused element: an outline of 2px or more that is drawn. */
async function ringVisible(page: Page) {
  return page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null;
    if (!el || el === document.body) return false;
    const s = getComputedStyle(el);
    return s.outlineStyle !== "none" && parseFloat(s.outlineWidth) >= 2;
  });
}

/** Tabs forward until the element with `testId` has the focus; every stop on the way has a ring. */
async function tabTo(page: Page, testId: string, max = 60) {
  for (let i = 0; i < max; i++) {
    if ((await focused(page)) === testId) {
      expect(await ringVisible(page), `focus ring on ${testId}`).toBe(true);
      return;
    }
    await page.keyboard.press("Tab");
    const now = await focused(page);
    // The cycle leaves the page for a moment at its end (the body has the focus); every other
    // stop, named or not, must show where the focus is.
    if (now?.startsWith("<body")) continue;
    expect(await ringVisible(page), `focus ring on ${now ?? "an unnamed stop"}`).toBe(true);
  }
  throw new Error(`Tab never reached ${testId}; stopped on ${await focused(page)}`);
}

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("login, plan picker, Home, Games, launch, Shop order, Account and Log out by keyboard only", async ({
  page,
}) => {
  await openKiosk(page);
  await emit(page, names.events.attract, ATTRACT);
  await expect(page.getByTestId("attract")).toBeVisible();

  // Login: any key opens the panel with the username focused; Tab moves on, Enter signs in.
  await page.keyboard.press("x");
  await expect(page.getByTestId("login-username")).toBeFocused();
  await page.keyboard.type("asha");
  await page.keyboard.press("Tab");
  await expect(page.getByTestId("login-password")).toBeFocused();
  await page.keyboard.type("right-password");
  await page.keyboard.press("Enter");

  // Plan picker: arrows move between the plans, Space or Enter chooses, Tab reaches Start.
  const options = page.getByTestId("picker-option");
  await expect(options.first()).toBeVisible();
  await tabTo(page, "picker-option");
  await page.keyboard.press("ArrowRight");
  await expect(options.nth(1)).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(options.nth(1)).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: "Start" }).first()).toBeFocused();
  await page.keyboard.press("Enter");
  expect(await calls(page)).toContain(names.commands.playerStart);

  // The Hub starts the session: the shell opens on Home.
  await emit(page, names.events.pcScreen, shown(PREPAID));
  await expect(page.getByTestId("shell")).toBeVisible();
  await expect(page.getByTestId("tab-home")).toHaveAttribute("aria-current", "page");

  // Pill nav: Tab lands on the roving item, arrows move, Enter selects.
  await tabTo(page, "tab-home");
  await page.keyboard.press("ArrowRight");
  await expect(page.getByTestId("tab-games")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("tab-games")).toHaveAttribute("aria-current", "page");

  // Games: the filter rail steps with Up and Down, the grid with all four arrows, Enter launches.
  await expect(page.getByTestId("games-grid")).toBeVisible();
  await tabTo(page, "filter-item");
  await page.keyboard.press("ArrowDown");
  await expect(page.getByTestId("filter-item").nth(1)).toBeFocused();
  await page.keyboard.press("ArrowUp");
  await expect(page.getByTestId("filter-item").first()).toBeFocused();
  await tabTo(page, "games-search");
  await page.keyboard.press("Tab");
  await expect(page.getByTestId("games-grid").locator("button").first()).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByTestId("games-grid").locator("button").nth(1)).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(page.getByTestId("games-grid").locator("button").nth(8)).toBeFocused(); // 7 columns at 1920
  await page.keyboard.press("Enter");
  await expect.poll(() => calls(page)).toContain(names.commands.gameLaunch);

  // Shop: reach it with the arrows; Add opens the cart with the focus inside it, Esc closes it and
  // gives the focus back to the Shop button, and Enter on that button opens it again.
  await tabTo(page, "tab-games");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await expect(page.getByTestId("tab-shop")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("shop-grid")).toBeVisible();
  await tabTo(page, "shop-item-add");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("cart")).toBeFocused();
  await expect(page.getByTestId("cart-line").first()).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("cart")).toHaveCount(0);
  await expect(page.getByTestId("shop-cart-button")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("cart")).toBeFocused();
  await tabTo(page, "cart-more");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("cart-qty").first()).toHaveText("2");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("cart")).toHaveCount(0);

  // Account: the chip opens the menu, arrows move between rows, Enter opens the page whose
  // sub-nav steps with the arrow keys.
  await tabTo(page, "account-chip");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("account-menu")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("account-menu")).toHaveCount(0);
  await expect(page.getByTestId("account-chip")).toBeFocused();
  await page.keyboard.press("Enter");
  const account = page.getByTestId("account-row-account");
  await page.keyboard.press("ArrowDown");
  for (let i = 0; i < 6 && (await focused(page)) !== "account-row-account"; i++) {
    await page.keyboard.press("ArrowDown");
  }
  await expect(account).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("account-page")).toBeVisible();
  const sub = page.getByTestId("account-subnav").getByRole("button");
  await tabTo(page, "account-nav-details");
  await page.keyboard.press("ArrowDown");
  await expect(sub.nth(1)).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("account-nav-wallet")).toHaveAttribute("aria-current", "page");

  // Log out: the menu row, the question (prepaid time is left) and its two buttons.
  await tabTo(page, "account-chip");
  await page.keyboard.press("Enter");
  for (let i = 0; i < 8 && (await focused(page)) !== "account-row-logout"; i++) {
    await page.keyboard.press("ArrowDown");
  }
  await expect(page.getByTestId("account-row-logout")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("account-logout-confirm")).toBeVisible();
  await expect(page.getByRole("button", { name: "Stay" })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByRole("button", { name: "Log out" })).toBeFocused();
  expect(await ringVisible(page)).toBe(true);
  await page.keyboard.press("Enter");
  await expect.poll(() => calls(page)).toContain(names.commands.playerEnd);
});

test("Shift+Tab walks back through the shell in reverse order", async ({ page }) => {
  await openKiosk(page);
  await emit(page, names.events.attract, ATTRACT);
  await emit(page, names.events.pcScreen, shown(PREPAID));
  await expect(page.getByTestId("shell")).toBeVisible();
  await tabTo(page, "account-chip");
  await page.keyboard.press("Shift+Tab");
  expect(await focused(page)).toBe("time-chip");
  expect(await ringVisible(page)).toBe(true);
  await page.keyboard.press("Shift+Tab");
  expect(await focused(page)).toBe("call-staff-button");
});

test("the player pages bind no other shortcut than the keys of the controls themselves", async ({
  page,
}) => {
  await openKiosk(page);
  await emit(page, names.events.attract, ATTRACT);
  await emit(page, names.events.pcScreen, shown(PREPAID));
  await expect(page.getByTestId("shell")).toBeVisible();
  // Letters, function keys and the Extend chord do nothing in the page: no tab, panel or call.
  await page.keyboard.press("e");
  await page.keyboard.press("g");
  await page.keyboard.press("F2");
  await page.keyboard.press("Control+Shift+E");
  await expect(page.getByTestId("tab-home")).toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("account-menu")).toHaveCount(0);
  expect(
    (await calls(page))
      .filter((c) => c !== names.commands.hudExpand && c !== names.commands.volumeGet)
      .filter((c) =>
        [names.commands.playerEnd, names.commands.playerLock, names.commands.callStaff].includes(c),
      ),
  ).toEqual([]);
});
