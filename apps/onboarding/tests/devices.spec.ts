import { expect, test, type Page } from "@playwright/test";
import type { BranchSetup, SetupCommandView, SetupDevice } from "@arena/ui/generated";
import { READY_OWNER, stubCloud, t, type Cloud } from "./stub";

type Key = Parameters<typeof t>[0];
const h1 = (page: Page, key: Key) => page.getByRole("heading", { level: 1, name: t(key) });
const START = new Date("2026-10-26T10:00:00Z");

const pc = (over: Partial<SetupDevice> = {}): SetupDevice => ({
  id: "D1",
  label: "PC-1",
  kind: "pc",
  group_id: null,
  status: "online",
  ...over,
});
const gaming = { id: "G1", name: "Gaming", kind: "pc", colour: "teal", sort: 0 };
const block = (over: Partial<BranchSetup> = {}): BranchSetup => ({
  devices: [pc()],
  groups: [gaming],
  ...over,
});
const rejected = (kind: string, payload: unknown, code: string): SetupCommandView => ({
  key: "01JREJECTED0000000000000000",
  kind,
  payload,
  status: "rejected",
  error: { code, message: "no" } as SetupCommandView["error"],
});

async function open(page: Page, setup: BranchSetup | null = block(), more = {}) {
  const cloud = await stubCloud(page, {
    signedIn: true,
    owner: READY_OWNER,
    wizard: {
      branch: {},
      done: ["cafe", "tier", "install_hub"],
      hubPaired: true,
      devices: { setup, reported_at: Date.now(), now: Date.now(), ...more },
    },
  });
  await page.goto("/setup/devices");
  await expect(h1(page, "ui.wizard.step.devices")).toBeVisible();
  return cloud;
}

test("an unassigned PC shows with its name and status", async ({ page }) => {
  await open(page);
  const row = page.getByTestId("unassigned-list").getByTestId("device-D1");
  await expect(row).toContainText("PC-1");
  await expect(row).toContainText(t("onboarding.devices.status.online"));
});

test("a PC that pairs later appears within 5 s without a reload", async ({ page }) => {
  await page.clock.install({ time: START });
  const cloud = await open(page, block({ devices: [] }));
  await expect(page.getByText(t("onboarding.devices.unassigned_empty"))).toBeVisible();
  cloud.deviceView.setup = block();
  await page.clock.runFor(5000);
  await expect(page.getByTestId("device-D1")).toBeVisible();
});

test("a new group is sent as group.upsert and shows as waiting for the branch", async ({
  page,
}) => {
  const cloud = await open(page);
  const form = page.getByTestId("group-form");
  await form.getByLabel(t("hub.groups.name")).fill("Console room");
  await form.getByLabel(t("hub.groups.kind")).selectOption("console");
  await form.getByLabel(t("hub.groups.colour.coral")).check();
  await form.getByRole("button", { name: t("hub.groups.add"), exact: true }).click();
  expect(cloud.devicePosts).toEqual([
    {
      kind: "group.upsert",
      payload: { name: "Console room", group_kind: "console", colour: "coral", sort_order: 1 },
    },
  ]);
  const group = page.getByTestId("group-list").getByRole("listitem").nth(1);
  await expect(group).toContainText("Console room");
  await expect(group).toContainText(t("onboarding.devices.waiting"));
});

test("Move to group sends device.assign and the device waits in the group", async ({ page }) => {
  const cloud = await open(page);
  const move = page.getByTestId("device-D1").getByLabel(t("onboarding.devices.move"));
  await move.selectOption("G1");
  await expect.poll(() => cloud.devicePosts.length).toBe(1);
  expect(cloud.devicePosts[0]).toEqual({
    kind: "device.assign",
    payload: { device_id: "D1", group_id: "G1" },
  });
  const row = page.getByTestId("group-G1").getByTestId("device-D1");
  await expect(row).toContainText(t("onboarding.devices.waiting"));
  await expect(page.getByTestId("unassigned-list").getByTestId("device-D1")).toHaveCount(0);
});

test("dragging a device onto a group sends device.assign", async ({ page }) => {
  const cloud = await open(page);
  await page.getByTestId("device-D1").dragTo(page.getByTestId("group-G1").locator(".drop").first());
  await expect.poll(() => cloud.devicePosts.length).toBe(1);
  expect(cloud.devicePosts[0].kind).toBe("device.assign");
});

test("a refused assignment returns the device with the plain reason inline", async ({ page }) => {
  await open(page, block(), {
    commands: [
      rejected("device.assign", { device_id: "D1", group_id: "G1" }, "GROUP_KIND_MISMATCH"),
    ],
  });
  const row = page.getByTestId("unassigned-list").getByTestId("device-D1");
  await expect(row).toContainText(t("onboarding.devices.reason.kind_mismatch"));
});

test("a console is added with a TV number and a full licence shows the tier-lock", async ({
  page,
}) => {
  const cloud = await open(page);
  const form = page.getByTestId("device-form");
  await form.getByLabel(t("hub.devices.label")).fill("PS5-1");
  await form.getByLabel(t("hub.devices.tv")).fill("3");
  await form.getByRole("button", { name: t("hub.devices.add"), exact: true }).click();
  expect(cloud.devicePosts).toEqual([
    {
      kind: "device.register",
      payload: { class: "paired", device_kind: "console", label: "PS5-1", tv_number: 3 },
    },
  ]);
  await expect(page.getByTestId("unassigned-list")).toContainText("PS5-1");
  cloud.deviceView.commands = [rejected("device.register", {}, "DEVICE_LIMIT")];
  await form.getByLabel(t("hub.devices.label")).fill("PS5-2");
  await form.getByLabel(t("hub.devices.tv")).fill("4");
  await form.getByRole("button", { name: t("hub.devices.add"), exact: true }).click();
  await expect(page.locator("[data-component=tier-lock]")).toContainText(
    t("staff.tier.available_on", { tier: "Basic" }),
  );
});

test("bad device details are refused on the page and nothing is sent", async ({ page }) => {
  const cloud = await open(page);
  await page
    .getByTestId("device-form")
    .getByRole("button", { name: t("hub.devices.add"), exact: true })
    .click();
  await expect(page.getByText(t("onboarding.devices.error.label"))).toBeVisible();
  await expect(page.getByText(t("onboarding.devices.error.tv"))).toBeVisible();
  expect(cloud.devicePosts).toEqual([]);
});

test("with the Hub offline the last devices show with their age and an offline badge", async ({
  page,
}) => {
  const now = Date.now();
  await open(page, block(), { hub_online: false, reported_at: now - 3 * 60_000 - 5000, now });
  const status = page.getByTestId("hub-offline");
  await expect(status).toContainText(t("onboarding.devices.hub_offline"));
  await expect(status).toContainText(t("onboarding.devices.as_of", { n: 3 }));
  await expect(page.getByTestId("device-D1")).toBeVisible();
});

test("step 4 completes by itself once a device is grouped and focus moves to step 5", async ({
  page,
}) => {
  await page.clock.install({ time: START });
  const cloud: Cloud = await open(page);
  cloud.deviceView.setup = block({ devices: [pc({ group_id: "G1" })] });
  cloud.done.add("devices");
  await page.clock.runFor(5000);
  await expect(page).toHaveURL(/\/setup\/rates$/);
  await expect(h1(page, "ui.wizard.step.rates")).toBeFocused();
});
