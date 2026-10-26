import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { BranchSetup, SetupDevice, SetupStatus } from "@arena/ui/generated";
import { READY_OWNER, stubCloud, t, type WizardStart } from "./stub";

type Key = Parameters<typeof t>[0];
const h1 = (page: Page, key: Key) => page.getByRole("heading", { level: 1, name: t(key) });
const pc = (id: string, over: Partial<SetupDevice> = {}): SetupDevice => ({
  id,
  label: `PC-${id}`,
  kind: "pc",
  group_id: "G1",
  status: "online",
  ...over,
});
const status = (test_session: SetupStatus["test_session"]): SetupStatus => ({
  opening_hours_set: false,
  backup_location_set: false,
  ad_consent: "missing",
  printer: "ok",
  products_count: 0,
  groups_without_rate: 0,
  manager_exists: false,
  test_session,
  live: false,
});
const block = (test_session: SetupStatus["test_session"] = "unset", devices = [pc("D1")]) =>
  ({
    devices,
    groups: [{ id: "G1", name: "Gaming", kind: "pc", colour: "teal", sort: 0, has_rate: true }],
    status: status(test_session),
  }) satisfies BranchSetup;

const BEFORE: WizardStart["done"] = [
  "cafe",
  "tier",
  "install_hub",
  "devices",
  "rates",
  "products",
  "staff",
  "printer_payments",
];

async function open(page: Page, setup: BranchSetup | null = block(), commands = []) {
  const cloud = await stubCloud(page, {
    signedIn: true,
    owner: READY_OWNER,
    wizard: {
      branch: {},
      done: BEFORE,
      hubPaired: true,
      devices: { setup, reported_at: Date.now(), now: Date.now(), commands },
    },
  });
  await page.goto("/setup/test_session");
  await expect(h1(page, "ui.wizard.step.test_session")).toBeVisible();
  return cloud;
}

const start = (page: Page) =>
  page.getByRole("button", { name: t("onboarding.test_session.start"), exact: true });

test("only PCs that are online and in a group are offered, and Start sends the command", async ({
  page,
}) => {
  const cloud = await open(
    page,
    block("unset", [
      pc("D1"),
      pc("D2", { status: "offline" }),
      pc("D3", { group_id: null }),
      pc("D4", { kind: "console" }),
      pc("D5"),
    ]),
  );
  const options = page.getByLabel(t("onboarding.test_session.pick"));
  await expect(options.locator("option")).toHaveText(["PC-D1", "PC-D5"]);
  await options.selectOption("D5");
  await start(page).click();
  await expect(page.getByTestId("test-session-state")).toHaveText(t("onboarding.devices.waiting"));
  expect(cloud.devicePosts).toEqual([{ kind: "setup.test_session", payload: { device_id: "D5" } }]);
});

test("with no PC online and grouped it says so and offers no Start", async ({ page }) => {
  await open(page, block("unset", [pc("D1", { group_id: null })]));
  await expect(page.getByTestId("test-session-none")).toHaveText(t("onboarding.test_session.none"));
  await expect(start(page)).toHaveCount(0);
});

test("a running test asks the owner to watch the PC", async ({ page }) => {
  await open(page, block("running"));
  await expect(page.getByTestId("test-session-state")).toHaveText(
    t("onboarding.test_session.running"),
  );
  await expect(start(page)).toHaveCount(0);
});

test("a failed test shows the retry line and Retry sends it again", async ({ page }) => {
  const cloud = await open(page, block("failed"));
  await expect(page.getByTestId("test-session-state")).toContainText(
    t("onboarding.test_session.failed"),
  );
  await page.getByRole("button", { name: t("common.retry"), exact: true }).click();
  expect(cloud.devicePosts).toEqual([{ kind: "setup.test_session", payload: { device_id: "D1" } }]);
});

test("a refused test shows its plain reason and keeps the list open", async ({ page }) => {
  await open(page, block(), [
    {
      key: "01JCOMMAND0000000000000001",
      kind: "setup.test_session",
      payload: { device_id: "D1" },
      status: "rejected",
      error: { code: "VALIDATION_FAILED", message: "the device is offline", field: "device_id" },
    },
  ] as never);
  await expect(page.getByTestId("test-session-refused")).toContainText("the device is offline");
  await expect(page.getByLabel(t("onboarding.test_session.pick"))).toBeVisible();
  await expect(start(page)).toBeVisible();
});

test("a passed test says the PC locked on cue, then the step advances by itself", async ({
  page,
}) => {
  await page.clock.install();
  const cloud = await open(page);
  await page.clock.pauseAt(Date.now() + 60_000);
  cloud.deviceView.setup = block("passed");
  cloud.done.add("test_session");
  await page.clock.runFor(5000);
  await expect(page.getByTestId("test-session-state")).toHaveText(
    t("onboarding.test_session.passed"),
  );
  await expect(page).toHaveURL(/\/setup\/test_session$/);
  await page.clock.runFor(4000);
  await expect(page).toHaveURL(/\/setup\/go_live$/);
  await expect(h1(page, "ui.wizard.step.go_live")).toBeFocused();
});

test("the step has no serious axe violations", async ({ page }) => {
  await open(page, block("failed"));
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
});
