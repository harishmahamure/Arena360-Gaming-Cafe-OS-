import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { ConnectivityState, HubSnapshot } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// The header's `chip-offline` (Story 1.22): the Hub's WAN probe says which link is down, in the
// snapshot and live as `event: connectivity.changed` on `/ui/events`. The chip names the link
// in words with a glyph, each change is announced once through the polite region, and the chip
// goes when the link comes back. The Hub's loopback API is stubbed with `page.route`.

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const STAFF = {
  staff_id: "01JMANAGER0000000000000000",
  display_name: "Manager",
  role_name: "Manager",
  permissions: ["settings.edit"],
};

type Hub = {
  link: ConnectivityState;
  /** When the probe found `link` (UTC ms). */
  at: number;
  hc: boolean;
  queue: string[];
  snapshots: number;
  /** Serve this as the snapshot's `connectivity` instead (`"omit"`: leave the field out). */
  snapshotConnectivity?: unknown;
  revoked?: boolean;
  licenceUnverified?: boolean;
  clockWrong?: boolean;
};

function snapshot(hub: Hub): HubSnapshot {
  const s: Record<string, unknown> = {
    branch: { name: "Dev Branch", branch_code: "DEV01" },
    devices: [],
    locale: null,
    ui: { idle_lock_minutes: 5, hc: hub.hc },
    connectivity: { state: hub.link, checked_at: hub.at },
  };
  if (hub.revoked) s.pairing_revoked = true;
  if (hub.licenceUnverified) s.licence_unverified = true;
  if (hub.clockWrong) s.clock_wrong = true;
  if (hub.snapshotConnectivity === "omit") delete s.connectivity;
  else if (hub.snapshotConnectivity !== undefined) s.connectivity = hub.snapshotConnectivity;
  return s as HubSnapshot;
}

/** Pushes `connectivity.changed` with this exact body. */
function push(hub: Hub, body: unknown) {
  hub.queue.push(`event: connectivity.changed\ndata: ${JSON.stringify(body)}\n\n`);
}

/** The Hub's probe found `link`: the snapshot says so and the stream pushes the change. */
function setLink(hub: Hub, link: ConnectivityState) {
  hub.link = link;
  hub.at += 1000;
  push(hub, { state: link, checked_at: hub.at });
}

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

async function open(
  page: Page,
  link: ConnectivityState = "online",
  hc = false,
  snapshotConnectivity?: unknown,
) {
  const hub: Hub = {
    link,
    at: 1_800_000_000_000,
    hc,
    queue: [],
    snapshots: 0,
    snapshotConnectivity,
  };
  await page.addInitScript(() => {
    sessionStorage.setItem("arena.hub.token", "manager-token");
    // Every text the polite region takes, in order.
    const seen: string[] = [];
    (window as unknown as { __announced: string[] }).__announced = seen;
    new MutationObserver(() => {
      const region = document.querySelector('[data-testid="hub-announce"]');
      const text = region?.textContent?.trim() ?? "";
      if (text && seen.at(-1) !== text) seen.push(text);
    }).observe(document, { subtree: true, childList: true, characterData: true });
  });
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    switch (url.pathname) {
      case "/ui/snapshot":
        hub.snapshots++;
        return json(route, 200, snapshot(hub));
      case "/ui/me":
        return json(route, 200, STAFF);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n` + hub.queue.splice(0).join(""),
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  await expect(page.locator("h1")).toHaveCount(1);
  return hub;
}

const chip = (page: Page) => page.getByTestId("hub-chip-offline");
const announced = (page: Page) =>
  page.evaluate(() => (window as unknown as { __announced: string[] }).__announced);

test("online: no chip and nothing announced", async ({ page }) => {
  await open(page);
  await expect(page.getByTestId("hub-staff")).toBeVisible();
  await page.waitForTimeout(800);
  await expect(chip(page)).toHaveCount(0);
  expect(await announced(page)).toEqual([]);
});

test("internet down: the chip says so with a glyph, announced once", async ({ page }) => {
  const hub = await open(page);
  setLink(hub, "internet_offline");
  await expect(chip(page)).toHaveText(`⚠ ${en["hub.connectivity.internet_offline"]}`);
  // The chip sits in the one header, before the clock.
  await expect(page.locator("header")).toHaveCount(1);
  await expect(page.locator("header .header-end > *").last()).not.toHaveAttribute(
    "data-testid",
    "hub-chip-offline",
  );
  await expect(page.getByTestId("hub-announce")).toHaveText(
    en["hub.connectivity.internet_offline"],
  );
  // Reconnects and snapshot reloads with the same state are not news.
  const before = hub.snapshots;
  await expect.poll(() => hub.snapshots, { timeout: 5000 }).toBeGreaterThan(before + 1);
  expect(await announced(page)).toEqual([en["hub.connectivity.internet_offline"]]);
  // The glyph is decorative: the words carry the state.
  await expect(chip(page).locator('[aria-hidden="true"]')).toHaveText("⚠");
});

test("cloud down: the chip says the cloud is unreachable", async ({ page }) => {
  const hub = await open(page);
  setLink(hub, "cloud_unreachable");
  await expect(chip(page)).toContainText(en["hub.connectivity.cloud_unreachable"]);
  await expect(chip(page)).toHaveAttribute("data-link", "cloud_unreachable");
  await expect(page.getByTestId("hub-announce")).toHaveText(
    en["hub.connectivity.cloud_unreachable"],
  );
});

test("a snapshot that already says offline shows the chip at once", async ({ page }) => {
  await open(page, "internet_offline");
  await expect(chip(page)).toContainText(en["hub.connectivity.internet_offline"]);
});

test("each change is announced once, and the chip goes when the link is back", async ({ page }) => {
  const hub = await open(page);
  setLink(hub, "internet_offline");
  await expect(chip(page)).toContainText(en["hub.connectivity.internet_offline"]);
  setLink(hub, "cloud_unreachable");
  await expect(chip(page)).toContainText(en["hub.connectivity.cloud_unreachable"]);
  setLink(hub, "online");
  await expect(chip(page)).toHaveCount(0);
  await expect(page.getByTestId("hub-announce")).toHaveText(en["hub.connectivity.restored"]);
  expect(await announced(page)).toEqual([
    en["hub.connectivity.internet_offline"],
    en["hub.connectivity.cloud_unreachable"],
    en["hub.connectivity.restored"],
  ]);
});

test("a snapshot older than a pushed change never puts the old state back", async ({ page }) => {
  const hub = await open(page, "cloud_unreachable");
  await expect(chip(page)).toHaveAttribute("data-link", "cloud_unreachable");
  // Every later snapshot is the stale one (as if it left the Hub before the probe ran).
  hub.snapshotConnectivity = { state: "cloud_unreachable", checked_at: hub.at };
  push(hub, { state: "internet_offline", checked_at: hub.at + 1000 });
  await expect(chip(page)).toHaveAttribute("data-link", "internet_offline");
  const before = hub.snapshots;
  await expect.poll(() => hub.snapshots, { timeout: 5000 }).toBeGreaterThan(before + 1);
  await expect(chip(page)).toHaveAttribute("data-link", "internet_offline");
  expect(await announced(page)).toEqual([
    en["hub.connectivity.cloud_unreachable"],
    en["hub.connectivity.internet_offline"],
  ]);
});

for (const [name, connectivity] of [
  ["without connectivity", "omit"],
  ["with an unknown state", { state: "on_fire", checked_at: 1_800_000_000_000 }],
] as const) {
  test(`a snapshot ${name} shows no chip and announces nothing`, async ({ page }) => {
    await open(page, "online", false, connectivity);
    await expect(page.getByTestId("hub-staff")).toBeVisible();
    await page.waitForTimeout(800);
    await expect(chip(page)).toHaveCount(0);
    expect(await announced(page)).toEqual([]);
  });
}

test("a pushed change with an unknown state is ignored", async ({ page }) => {
  const hub = await open(page);
  push(hub, { state: "on_fire", checked_at: hub.at + 1000 });
  const before = hub.snapshots;
  await expect.poll(() => hub.snapshots, { timeout: 5000 }).toBeGreaterThan(before + 1);
  await expect(chip(page)).toHaveCount(0);
  expect(await announced(page)).toEqual([]);
});

for (const theme of ["dark", "hc"] as const) {
  test(`the chip uses the chip-offline tokens and passes axe (${theme})`, async ({ page }) => {
    // The theme comes from the snapshot, as on a real Hub (applied before the first render).
    await open(page, "cloud_unreachable", theme === "hc");
    await expect(chip(page)).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.dataset.theme)).toBe(theme);
    const [bg, fg, tokenBg, tokenFg] = await chip(page).evaluate((el) => {
      const probe = document.createElement("span");
      probe.style.backgroundColor = "var(--chip-offline-background)";
      probe.style.color = "var(--chip-offline-foreground)";
      el.parentElement!.appendChild(probe);
      const p = getComputedStyle(probe);
      const out = [
        getComputedStyle(el).backgroundColor,
        getComputedStyle(el).color,
        p.backgroundColor,
        p.color,
      ];
      probe.remove();
      return out;
    });
    expect([bg, fg]).toEqual([tokenBg, tokenFg]);
    const results = await new AxeBuilder({ page }).analyze();
    const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
    expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
      [],
    );
  });
}

test("a revoked Hub shows a status banner and still shows the screen", async ({ page }) => {
  const hub = await open(page);
  const banner = page.getByTestId("hub-revoked");
  await expect(banner).toHaveCount(0);
  hub.revoked = true;
  await expect(banner).toHaveText(`⚠ ${en["hub.pairing.revoked"]}`);
  await expect(banner).toHaveAttribute("role", "status");
  await expect(page.locator("h1")).toHaveCount(1);
});

test("licence and clock warnings share one header status-badge slot until they clear", async ({
  page,
}) => {
  const hub = await open(page);
  const slot = page.getByTestId("hub-status-slot");
  await expect(slot).toHaveCount(0);
  hub.licenceUnverified = true;
  await expect(slot).toHaveAttribute("role", "status");
  await expect(slot.locator('[data-component="status-badge"]')).toHaveText([
    en["hub.licence.unverified"],
  ]);
  hub.clockWrong = true;
  await expect(slot.locator('[data-component="status-badge"]')).toHaveText([
    en["hub.licence.unverified"],
    en["hub.clock.wrong"],
  ]);
  hub.licenceUnverified = false;
  hub.clockWrong = false;
  await expect(slot).toHaveCount(0);
  await expect(page.locator("h1")).toHaveCount(1);
});
