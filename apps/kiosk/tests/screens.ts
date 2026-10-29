import { expect, type Page } from "@playwright/test";
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import { ATTRACT, PREPAID, emit, openKiosk, openShell, shown } from "./support";

// The player screens of Story 6.37, each as a way to reach it on the stubbed shell: the reflow,
// keyboard and high-contrast suites walk the same list.

const hudPrepaid = (over: Record<string, unknown> = {}) =>
  shown({ ...PREPAID, warning_min: null, ...over });

async function openLogin(page: Page) {
  await openKiosk(page);
  await expect(page.getByTestId("attract")).toBeVisible();
  await page.keyboard.press("x");
  await expect(page.getByTestId("login-panel")).toBeVisible();
}

export const SCREENS: Record<string, (page: Page) => Promise<void>> = {
  login: openLogin,
  "sign-up": async (page) => {
    await openLogin(page);
    await page.getByTestId("login-panel").getByRole("button", { name: "Create account" }).click();
    await expect(page.getByTestId("signup-form")).toBeVisible();
  },
  "plan picker": async (page) => {
    await openLogin(page);
    await page.getByTestId("login-username").fill("asha");
    await page.getByTestId("login-password").fill("right-password");
    await page.getByTestId("login-password").press("Enter");
    await expect(page.getByTestId("picker-option").first()).toBeVisible();
  },
  home: async (page) => {
    await openShell(page);
    await expect(page.getByTestId("featured-row")).toBeVisible();
  },
  games: async (page) => {
    await openShell(page, "games");
    await expect(page.getByTestId("games-grid")).toBeVisible();
  },
  apps: async (page) => {
    await openShell(page, "apps");
    await expect(page.getByTestId("apps")).toBeVisible();
  },
  shop: async (page) => {
    await openShell(page);
    await expect(page.getByTestId("tab-shop")).toBeVisible();
    await page.getByTestId("tab-shop").click();
    await expect(page.getByTestId("shop-grid")).toBeVisible();
  },
  "shop with cart": async (page) => {
    await openShell(page);
    await page.getByTestId("tab-shop").click();
    const add = page.getByTestId("shop-grid").locator("button.add").first();
    await add.click();
    await page.getByTestId("shop-cart-button").click();
    await expect(page.getByTestId("cart")).toBeVisible();
    await expect(page.getByTestId("cart-line").first()).toBeVisible();
  },
  account: async (page) => {
    await openShell(page);
    await page.getByTestId("account-chip").click();
    await page.getByTestId("account-row-account").click();
    await expect(page.getByTestId("account-page")).toBeVisible();
  },
  hud: async (page) => {
    await openKiosk(page, { path: "/hud" });
    await emit(page, names.events.attract, ATTRACT);
    await emit(page, names.events.pcScreen, hudPrepaid());
    await expect(page.getByTestId("hud-pill")).toBeVisible();
  },
  "warning toast": async (page) => {
    await openKiosk(page, { path: "/hud" });
    await emit(page, names.events.attract, ATTRACT);
    await emit(page, names.events.pcScreen, hudPrepaid({ warning_min: 10, remaining_ms: 600_000 }));
    await expect(page.getByTestId("toast")).toBeVisible();
  },
  "grace lock": async (page) => {
    await openKiosk(page, { path: "/hud" });
    await emit(page, names.events.attract, ATTRACT);
    await emit(page, names.events.pcScreen, hudPrepaid({ remaining_ms: 0, grace_in_ms: 0 }));
    await expect(page.getByTestId("grace")).toBeVisible();
  },
};
