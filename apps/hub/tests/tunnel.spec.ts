import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { HubSnapshot, SessionStaff, TunnelLine } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { parseTunnelLine } from "../src/lib/tunnel";
import { ORIGIN } from "./origin";

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const MANAGER: SessionStaff = {
  staff_id: "01JMANAGER0000000000000000",
  display_name: "Manager",
  role_name: "Manager",
  permissions: ["settings.edit"],
};

function snapshot(): HubSnapshot {
  return {
    sessions: [],
    branch: { name: "Dev Branch", branch_code: "DEV01" },
    groups: [],
    zones: [],
    devices: [],
    locale: null,
    ui: { idle_lock_minutes: 5, hc: false },
    connectivity: { state: "online", checked_at: null },
    setup: null,
    lan: null,
    paired: true,
  } as unknown as HubSnapshot;
}

/** Serves the Hub's loopback API with `line` as the tunnel state (`null`: the Hub is down). */
async function stub(page: Page, line: () => TunnelLine | null) {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "manager-token"));
  await page.route(`${HUB}/**`, (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const json = (status: number, body: unknown) =>
      route.fulfill({
        status,
        contentType: "application/json",
        headers: CORS,
        body: JSON.stringify(body),
      });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(200, snapshot());
      case "/ui/me":
        return json(200, MANAGER);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 60000\n\n",
        });
      case "/ui/tunnel": {
        const current = line();
        return current ? json(200, current) : route.abort();
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
}

test("a Windows Hub with the tunnel shows it on, with the branch address", async ({ page }) => {
  await stub(page, () => ({ state: "on", hostname: "B1.tunnel.arena360.cloud" }));
  await page.goto("/system/connection");
  await expect(page.getByRole("heading", { level: 2 })).toHaveText(en["hub.tunnel.title"]);
  const row = page.getByTestId("tunnel-line");
  await expect(row).toContainText(en["hub.tunnel.state.on"]);
  await expect(page.getByTestId("tunnel-hostname")).toHaveText("Address: B1.tunnel.arena360.cloud");
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test("an Android Hub reads Needs the Windows Hub and shows no address", async ({ page }) => {
  await stub(page, () => ({ state: "unsupported_platform" }));
  await page.goto("/system/connection");
  await expect(page.getByTestId("tunnel-line")).toContainText("Needs the Windows Hub");
  await expect(page.getByTestId("tunnel-hostname")).toHaveCount(0);
});

test("a connector that dropped reads reconnecting, then on again", async ({ page }) => {
  let line: TunnelLine = { state: "reconnecting", hostname: "B1.tunnel.arena360.cloud" };
  await stub(page, () => line);
  await page.goto("/system/connection");
  await expect(page.getByTestId("tunnel-line")).toContainText(en["hub.tunnel.state.reconnecting"]);
  line = { state: "on", hostname: "B1.tunnel.arena360.cloud" };
  await expect(page.getByTestId("tunnel-line")).toContainText(en["hub.tunnel.state.on"], {
    timeout: 15_000,
  });
});

test("a Hub that cannot be read says so", async ({ page }) => {
  await stub(page, () => null);
  await page.goto("/system/connection");
  await expect(page.getByTestId("tunnel-unreadable")).toHaveText(en["hub.tunnel.unreadable"]);
});

test("the System sections list Connection", async ({ page }) => {
  await stub(page, () => ({ state: "off" }));
  await page.goto("/system/connection");
  await expect(
    page.getByRole("navigation", { name: en["hub.system.sections"] }).getByRole("link", {
      name: en["hub.system.nav.connection"],
    }),
  ).toHaveAttribute("aria-current", "page");
});

test("the line parser accepts the Hub's states and nothing else", () => {
  expect(parseTunnelLine({ state: "off" })).toEqual({ state: "off", hostname: undefined });
  expect(parseTunnelLine({ state: "on", hostname: "h" })).toEqual({ state: "on", hostname: "h" });
  for (const bad of [null, "on", {}, { state: "bogus" }, { state: "on", hostname: 7 }]) {
    expect(parseTunnelLine(bad)).toBeNull();
  }
});
