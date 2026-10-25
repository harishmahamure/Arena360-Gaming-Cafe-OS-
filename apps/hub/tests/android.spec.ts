import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { DeviceSpec, HubSnapshot } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { MIN_WEBVIEW_MAJOR, gbOf, loadDevice, webviewMajor } from "../src/lib/android";
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const UNPAIRED: HubSnapshot = {
  branch: null,
  groups: [],
  zones: [],
  devices: [],
  locale: null,
  ui: { idle_lock_minutes: 5, hc: true },
  connectivity: { state: "online", checked_at: null },
  setup: null,
  lan: null,
  paired: false,
};
const ua = (chrome: number) =>
  `Mozilla/5.0 (Linux; Android 14; SM-X110) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${chrome}.0.0.0 Safari/537.36`;
const FAIL: DeviceSpec = {
  failures: [
    { requirement: "ram", found_bytes: 2_100_000_000, needed_gb: 3 },
    { requirement: "storage", found_bytes: 13_960_000_000, needed_gb: 20 },
  ],
};
const PASS: DeviceSpec = { failures: [] };

test.describe("device helpers", () => {
  test("reads the WebView major from the user agent", () => {
    expect(webviewMajor(ua(126))).toBe(126);
    expect(webviewMajor("Mozilla/5.0 Firefox/130.0")).toBeNull();
  });

  test("converts bytes to decimal GB with at most one decimal", () => {
    expect(gbOf(13_960_000_000)).toBe(14);
    expect(gbOf(2_840_000_000)).toBe(2.8);
    expect(gbOf(0)).toBe(0);
  });

  test("a non-Android user agent never asks the shell", async () => {
    const invoke = () => Promise.reject(new Error("must not be called"));
    expect(await loadDevice("Mozilla/5.0 (Windows NT 10.0) Chrome/90.0", invoke)).toEqual({
      kind: "ok",
    });
  });

  test("a failing spec wins over an old WebView", async () => {
    const d = await loadDevice(ua(90), () => Promise.resolve(FAIL));
    expect(d).toEqual({ kind: "spec", failures: FAIL.failures });
  });

  test("an old WebView is flagged only below the minimum", async () => {
    const pass = () => Promise.resolve(PASS);
    expect(await loadDevice(ua(MIN_WEBVIEW_MAJOR - 1), pass)).toEqual({
      kind: "webview",
      major: MIN_WEBVIEW_MAJOR - 1,
    });
    expect(await loadDevice(ua(MIN_WEBVIEW_MAJOR), pass)).toEqual({ kind: "ok" });
  });

  test("a shell that errors or answers nonsense does not block the Hub", async () => {
    expect(await loadDevice(ua(126), () => Promise.reject(new Error("x")))).toEqual({ kind: "ok" });
    expect(await loadDevice(ua(90), () => Promise.reject(new Error("x")))).toEqual({
      kind: "webview",
      major: 90,
    });
    expect(await loadDevice(ua(126), () => Promise.resolve(null))).toEqual({ kind: "ok" });
    expect(await loadDevice(ua(126), null)).toEqual({ kind: "ok" });
  });
});

async function stubShell(page: Page, first: DeviceSpec) {
  await page.addInitScript((spec: DeviceSpec) => {
    const w = window as unknown as { __calls: number; __spec: DeviceSpec; __TAURI__: unknown };
    w.__calls = 0;
    w.__spec = spec;
    w.__TAURI__ = { core: { invoke: async () => (w.__calls++, w.__spec) } };
  }, first);
  const snapshots = { n: 0 };
  await page.route(`${HUB}/ui/**`, (route) => {
    if (route.request().method() === "OPTIONS")
      return route.fulfill({ status: 204, headers: CORS });
    if (route.request().url().endsWith("/ui/snapshot")) snapshots.n++;
    return route.fulfill({
      status: route.request().url().endsWith("/ui/snapshot") ? 200 : 409,
      contentType: "application/json",
      headers: CORS,
      body: JSON.stringify(
        route.request().url().endsWith("/ui/snapshot")
          ? UNPAIRED
          : { code: "HUB_NOT_PAIRED", message: "hub not paired" },
      ),
    });
  });
  return snapshots;
}

const check = (page: Page) =>
  page.getByRole("button", { name: en["hub.device.check_again"], exact: true });

test.describe("a device below the spec", () => {
  test.use({ userAgent: ua(126) });

  test("names each failed requirement with the value found and loads nothing else", async ({
    page,
  }) => {
    const snapshots = await stubShell(page, FAIL);
    await page.goto("/");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["hub.device.title"]);
    await expect(page.getByTestId("device-ram")).toHaveText(
      "This device has 2.1 GB of RAM. The Hub needs 3 GB of RAM.",
    );
    await expect(page.getByTestId("device-storage")).toHaveText(
      "This device has 14 GB free. The Hub needs 20 GB free.",
    );
    await expect(page.locator("nav")).toHaveCount(0);
    await expect(page).not.toHaveURL(/pair/);
    expect(snapshots.n).toBe(0);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  });

  test("Check again goes straight to the pairing screen once the device passes", async ({
    page,
  }) => {
    await stubShell(page, FAIL);
    await page.goto("/");
    await expect(check(page)).toBeVisible();
    await page.evaluate(
      (spec) => ((window as never as { __spec: DeviceSpec }).__spec = spec),
      PASS,
    );
    await check(page).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["hub.pair.title"]);
    await expect(page).toHaveURL(/\/pair$/);
  });

  test("Check again keeps the screen while the device still fails", async ({ page }) => {
    await stubShell(page, FAIL);
    await page.goto("/");
    await check(page).click();
    await expect(page.getByTestId("device-ram")).toBeVisible();
    await expect
      .poll(() => page.evaluate(() => (window as never as { __calls: number }).__calls))
      .toBeGreaterThanOrEqual(2);
  });
});

test.describe("an Android System WebView below the minimum", () => {
  test.use({ userAgent: ua(MIN_WEBVIEW_MAJOR - 1) });

  test("asks for an update and does not load the Hub UI data", async ({ page }) => {
    const snapshots = await stubShell(page, PASS);
    await page.goto("/");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["hub.webview.title"]);
    await expect(page.getByTestId("device-webview")).toContainText("Android System WebView");
    await expect(page.getByTestId("device-webview")).toContainText(String(MIN_WEBVIEW_MAJOR));
    expect(snapshots.n).toBe(0);
    expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  });
});

test.describe("a device that meets the spec", () => {
  test.use({ userAgent: ua(126) });

  test("goes straight to Pair this Hub", async ({ page }) => {
    await stubShell(page, PASS);
    await page.goto("/");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["hub.pair.title"]);
  });
});
