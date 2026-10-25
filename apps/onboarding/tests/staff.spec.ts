import { expect, test, type Page } from "@playwright/test";
import type { StaffInviteView } from "@arena/ui/generated";
import { READY_OWNER, STAFF_CODES, stubCloud, t } from "./stub";

type Key = Parameters<typeof t>[0];
const BEFORE = ["cafe", "tier", "install_hub", "devices", "rates", "products"] as const;
const grouped = (code: string) => `${code.slice(0, 4)} ${code.slice(4)}`;
const h1 = (page: Page, key: Key) => page.getByRole("heading", { level: 1, name: t(key) });
const button = (page: Page, key: Key) => page.getByRole("button", { name: t(key), exact: true });

const invite = (over: Partial<StaffInviteView> = {}): StaffInviteView => ({
  id: "01JINVITE0000000000000000A",
  name: "Neha",
  phone: "+919876543211",
  role: "manager",
  status: "applied",
  error: null,
  setup_expires_at: Date.UTC(2026, 9, 28),
  ...over,
});

async function open(page: Page, wizard: { invites?: StaffInviteView[]; hubPaired?: boolean } = {}) {
  const cloud = await stubCloud(page, {
    signedIn: true,
    owner: READY_OWNER,
    wizard: { branch: {}, done: [...BEFORE], hubPaired: true, ...wizard },
  });
  await page.goto("/setup/staff");
  await expect(h1(page, "ui.wizard.step.staff")).toBeVisible();
  return cloud;
}

async function fill(page: Page, name: string, phone: string) {
  await page.getByLabel(t("onboarding.staff.name")).fill(name);
  await page.getByLabel(t("onboarding.staff.phone")).fill(phone);
}

test("step 7 shows the form with Manager preselected, an empty list and Skip for now", async ({
  page,
}) => {
  await open(page);
  await expect(page.getByLabel(t("onboarding.staff.role.manager"))).toBeChecked();
  await expect(page.getByText(t("onboarding.staff.empty"))).toBeVisible();
  await expect(button(page, "onboarding.staff.invite")).toBeVisible();
  await expect(button(page, "onboarding.staff.skip")).toBeVisible();
});

test("an invite shows the setup code once with Copy, lists the invite as waiting and then preselects Counter", async ({
  page,
}) => {
  const cloud = await open(page);
  await fill(page, "Neha", "+91 98765 43211");
  await button(page, "onboarding.staff.invite").click();
  const notice = page.getByTestId("staff-code-notice");
  await expect(page.getByTestId("staff-code-text")).toHaveText(
    t("staff.setup_code.issued", { name: "Neha", code: grouped(STAFF_CODES[0]) }),
  );
  await expect(notice.getByRole("button", { name: t("ui.pairing_code.copy") })).toBeVisible();
  expect(cloud.invitePosts).toEqual([{ name: "Neha", phone: "+919876543211", role: "manager" }]);
  const row = page.getByTestId("staff-list").getByRole("listitem");
  await expect(row).toContainText(t("onboarding.staff.status.waiting"));
  await expect(row).toContainText(t("onboarding.staff.login", { phone: "+919876543211" }));
  await expect(page.getByLabel(t("onboarding.staff.role.counter"))).toBeChecked();
  await expect(page.getByLabel(t("onboarding.staff.name"))).toHaveValue("");
});

test("bad details are refused on the page and nothing is sent", async ({ page }) => {
  const cloud = await open(page);
  await fill(page, "", "9876543211");
  await button(page, "onboarding.staff.invite").click();
  await expect(page.getByText(t("onboarding.staff.error.name"))).toBeVisible();
  await expect(page.getByText(t("onboarding.staff.error.phone"))).toBeVisible();
  expect(cloud.invitePosts).toEqual([]);
});

test("a refused invite says it couldn't apply and why, and a full licence shows the tier-lock", async ({
  page,
}) => {
  await open(page, {
    invites: [
      invite({
        id: "01JINVITE0000000000000000A",
        status: "failed",
        error: { code: "LOGIN_TAKEN", message: "x", field: "login" },
      }),
      invite({
        id: "01JINVITE0000000000000000B",
        name: "Ravi",
        role: "counter",
        status: "failed",
        error: { code: "USER_LIMIT", message: "x" },
      }),
    ],
  });
  const taken = page.getByTestId("invite-01JINVITE0000000000000000A");
  await expect(taken).toContainText(t("onboarding.staff.status.failed"));
  await expect(taken).toContainText(t("onboarding.staff.reason.login_taken"));
  const full = page.getByTestId("invite-01JINVITE0000000000000000B");
  await expect(full).toContainText(t("onboarding.staff.reason.user_limit"));
  await expect(full.locator('[data-component="tier-lock"]')).toContainText(
    t("staff.tier.available_on", { tier: "Basic" }),
  );
  await expect(full.getByRole("link")).toHaveCount(0);
});

test("New code shows a fresh code once", async ({ page }) => {
  const cloud = await open(page, { invites: [invite({ status: "waiting" })] });
  await page
    .getByTestId("invite-01JINVITE0000000000000000A")
    .getByRole("button", {
      name: t("onboarding.staff.new_code"),
    })
    .click();
  await expect(page.getByTestId("staff-code-text")).toHaveText(
    t("staff.setup_code.issued", { name: "Neha", code: grouped(STAFF_CODES[2]) }),
  );
  expect(cloud.reissued).toEqual(["01JINVITE0000000000000000A"]);
});

test("Skip for now moves on to the next step", async ({ page }) => {
  const cloud = await open(page);
  await button(page, "onboarding.staff.skip").click();
  await expect(h1(page, "ui.wizard.step.printer_payments")).toBeVisible();
  expect([...cloud.skipped]).toEqual(["staff"]);
});

test("the wizard moves on by itself once a Manager invite is applied at the Hub", async ({
  page,
}) => {
  await page.clock.install();
  const cloud = await open(page, { invites: [invite({ status: "waiting" })] });
  cloud.invites[0].status = "applied";
  cloud.done.add("staff");
  await page.clock.runFor(5000);
  await expect(h1(page, "ui.wizard.step.printer_payments")).toBeVisible();
});
