import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { LiveFloor } from "@arena/ui/generated";
import { PAID, stubCloud, t, type Branch } from "./stub";

const HOST = "b1.tunnel.arena360.cloud";
const URL = `wss://${HOST}/live`;
const WINDOWS: Branch = { ...PAID[0], hub: { paired: true, platform: "windows" } };
const ANDROID: Branch = { ...PAID[0], hub: { paired: true, platform: "android" } };
const NO_FLAG: Branch = { ...PAID[0], flags: [] };

const floorNow = (): LiveFloor => ({
  generated_at: Date.now(),
  zones: [{ id: "z1", name: "Front room" }],
  tiles: [
    {
      device_id: "d1",
      label: "PC 1",
      zone: "z1",
      state: "active",
      session: { started_at: Date.now() - 65_000, paused_total_ms: 0, guest_no: 3 },
    },
    { device_id: "d2", label: "PC 2", zone: "z1", state: "online" },
    {
      device_id: "d3",
      label: "PC 3",
      zone: "z1",
      state: "active",
      session: {
        started_at: Date.now() - 600_000,
        paused_total_ms: 0,
        ends_at: Date.now() + 5 * 60_000,
        player_id: "01JPLAYER",
      },
    },
    { device_id: "d4", label: "PC 4", state: "offline" },
    { device_id: "d5", label: "PC 5", state: "online", remote: true },
  ],
});

const event = (floor: LiveFloor) =>
  JSON.stringify({ v: 1, kind: "event", rid: "ev", event: { kind: "live.floor", ...floor } });

type Sent = Record<string, unknown>;

/** A Hub behind the tunnel: admits the viewer, pushes the floor, acks token refreshes. */
async function hub(
  page: Page,
  opts: { close?: boolean } = {},
): Promise<{ frames: Sent[]; push: (f: LiveFloor) => void }> {
  const frames: Sent[] = [];
  let socket: { send: (m: string) => void } | null = null;
  await page.routeWebSocket(URL, (ws) => {
    socket = ws;
    ws.onMessage((m) => {
      const f = JSON.parse(String(m)) as Sent;
      frames.push(f);
      if (f.kind === "hello") {
        if (opts.close) return ws.close();
        ws.send(
          JSON.stringify({
            v: 1,
            kind: "hello.ok",
            rid: f.rid,
            class: "live_view",
            versions: { app: "0.1.0" },
            primary: true,
          }),
        );
        ws.send(event(floorNow()));
      } else if (f.kind === "live.token")
        ws.send(JSON.stringify({ v: 1, kind: "ack", rid: f.rid }));
    });
  });
  return { frames, push: (f) => socket?.send(event(f)) };
}

let issued = 0;
const grant = (branch: string) => ({
  status: 200,
  body: {
    url: `wss://${branch}.tunnel.arena360.cloud/live`,
    token: `tok-${++issued}`,
    expires_at: Date.now() + 300_000,
  },
});

test("the floor shows read-only tiles by zone, live, with the player id or Guest", async ({
  page,
}) => {
  issued = 0;
  const calls = await stubCloud(page, [WINDOWS], { liveView: grant });
  const fake = await hub(page);
  await page.goto("/live");

  await expect(page.getByRole("heading", { name: "Front room" })).toBeVisible();
  await expect(page.getByRole("heading", { name: t("hub.floor.unassigned") })).toBeVisible();
  const tile = (id: string) => page.locator(`[data-device="${id}"]`);
  await expect(tile("d1")).toContainText("PC 1");
  await expect(tile("d1")).toContainText(t("ui.status.in_session"));
  await expect(tile("d1")).toContainText(/0:01:\d\d/);
  await expect(tile("d1")).toContainText(t("hub.session.guest", { n: 3 }));
  await expect(tile("d3")).toContainText(t("owner.live.player", { id: "01JPLAYER" }));
  await expect(tile("d3")).toContainText(/0:0[45]:\d\d/);
  await expect(tile("d2")).toContainText(t("ui.status.free"));
  await expect(tile("d4")).toContainText(t("ui.status.offline"));
  await expect(tile("d5")).toContainText(t("hub.floor.remote"));
  await expect(tile("d2")).not.toContainText(t("hub.floor.remote"));
  await expect(page.getByTestId("live-age")).toContainText(t("owner.live.live"));
  // Read only: no buttons on a tile, and nothing but the hello was sent.
  await expect(page.locator("[data-device] button")).toHaveCount(0);
  expect(fake.frames).toHaveLength(1);
  expect(fake.frames[0]).toMatchObject({ kind: "hello", credential: "tok-1", class: "live_view" });
  expect(calls.bodies["live"]).toEqual(["b1"]);
});

test("a new floor frame replaces the tiles and an old one shows its age", async ({ page }) => {
  await stubCloud(page, [WINDOWS], { liveView: grant });
  const fake = await hub(page);
  await page.goto("/live");
  await expect(page.locator('[data-device="d2"]')).toContainText(t("ui.status.free"));
  fake.push({
    ...floorNow(),
    tiles: [{ device_id: "d2", label: "PC 2", state: "maintenance" }],
    zones: [],
  });
  await expect(page.locator('[data-device="d2"]')).toContainText(t("ui.status.maintenance"));
  await expect(page.locator('[data-device="d1"]')).toHaveCount(0);
  // No frame for a few seconds: the age is spelled out.
  await expect(page.getByTestId("live-age")).toContainText(/Updated \d+ s ago/, {
    timeout: 10_000,
  });
});

test("the token is renewed after 4 minutes over the open socket", async ({ page }) => {
  issued = 0;
  await page.clock.install();
  await stubCloud(page, [WINDOWS], { liveView: grant });
  const fake = await hub(page);
  await page.goto("/live");
  await expect(page.locator('[data-device="d1"]')).toBeVisible();
  expect(fake.frames.map((f) => f.kind)).toEqual(["hello"]);
  await page.clock.fastForward(4 * 60_000 + 1_000);
  await expect.poll(() => fake.frames.map((f) => f.kind)).toEqual(["hello", "live.token"]);
  expect(fake.frames[1]).toMatchObject({ token: "tok-2" });
  // The view never dropped.
  await expect(page.locator('[data-device="d1"]')).toBeVisible();
});

test("without the tunnel flag Live view is the tier lock, and so is the cloud's refusal", async ({
  page,
}) => {
  const calls = await stubCloud(page, [NO_FLAG]);
  await page.goto("/live");
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Premium" }))).toBeVisible();
  await expect(page.getByRole("link", { name: t("staff.tier.upgrade") })).toHaveCount(0);
  expect(calls.bodies["live"]).toBeUndefined();

  // The licence moved on since the branch list was read: the cloud says so.
  await page.unroute("**/v1/**");
  await stubCloud(page, [WINDOWS], {
    liveView: () => ({ status: 403, body: { code: "FEATURE_NOT_ENTITLED", message: "" } }),
  });
  await page.goto("/live");
  await expect(page.getByText(t("staff.tier.available_on", { tier: "Premium" }))).toBeVisible();
});

test("an Android Hub reads Needs the Windows Hub on the card and in Live view", async ({
  page,
}) => {
  const calls = await stubCloud(page, [ANDROID]);
  await page.goto("/");
  const card = page.getByRole("region", { name: ANDROID.name });
  await expect(card.getByText(t("owner.live.needs_windows"))).toBeVisible();
  await expect(card.getByRole("link", { name: t("owner.live.open") })).toHaveCount(0);
  await page.goto("/live");
  await expect(page.getByText(t("owner.live.needs_windows"))).toBeVisible();
  expect(calls.bodies["live"]).toBeUndefined();
});

test("a Hub that cannot be reached through the tunnel shows Branch offline and a way back", async ({
  page,
}) => {
  await stubCloud(page, [WINDOWS], { liveView: grant });
  await hub(page, { close: true });
  await page.goto("/live");
  await expect(page.getByText(t("owner.today.offline"))).toBeVisible();
  const back = page.getByRole("link", { name: t("owner.live.back") });
  await expect(back).toHaveAttribute("href", "/#today-b1");
  await back.click();
  await expect(page).toHaveURL(/\/#today-b1$/);

  // The cloud has no tunnel for it yet.
  await page.unroute("**/v1/**");
  await stubCloud(page, [WINDOWS], {
    liveView: () => ({ status: 503, body: { code: "NO_HUB", message: "" } }),
  });
  await page.goto("/live");
  await expect(page.getByText(t("owner.today.offline"))).toBeVisible();
});

test("the sidebar carries Live view on desktop, the Today card on a phone", async ({
  page,
}, info) => {
  await stubCloud(page, [WINDOWS], { liveView: grant });
  await hub(page);
  await page.goto("/");
  const side = page.getByRole("navigation", { name: t("owner.nav.label") });
  const card = page.getByRole("region", { name: WINDOWS.name });
  if (info.project.name === "desktop") {
    await expect(side.getByRole("link", { name: t("owner.nav.live") })).toBeVisible();
    await expect(card.getByRole("link", { name: t("owner.live.open") })).toBeHidden();
    await side.getByRole("link", { name: t("owner.nav.live") }).click();
  } else {
    await expect(side.getByRole("link", { name: t("owner.nav.live") })).toBeHidden();
    await card.getByRole("link", { name: t("owner.live.open") }).click();
  }
  await expect(page).toHaveURL(/\/live/);
  await expect(page.locator('[data-device="d1"]')).toBeVisible();
});

test("the live floor has no serious accessibility violations", async ({ page }) => {
  await stubCloud(page, [WINDOWS], { liveView: grant });
  await hub(page);
  await page.goto("/live");
  await expect(page.locator('[data-device="d1"]')).toBeVisible();
  const blocking = (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );
  const report = blocking.map(
    (v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
  );
  expect(report, report.join("\n")).toEqual([]);
});
