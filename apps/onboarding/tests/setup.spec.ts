import { expect, test, type Page } from "@playwright/test";
import { GSTIN, PASSWORD, READY_OWNER, err, stubCloud, t } from "./stub";

type Key = Parameters<typeof t>[0];
const h1 = (page: Page, key: Key) => page.getByRole("heading", { level: 1, name: t(key) });
const button = (page: Page, key: Key) => page.getByRole("button", { name: t(key), exact: true });
const field = (page: Page, key: Key) => page.getByLabel(t(key), { exact: true });
const nav = (page: Page) => page.getByRole("navigation", { name: t("ui.wizard.label") });
const ready = { signedIn: true, owner: READY_OWNER };
const created = { ...ready, wizard: { branch: {} } };

const isWide = (page: Page) => (page.viewportSize()?.width ?? 0) >= 840;

test.describe("progress", () => {
  test("at desktop width: ten rows, done rows link, later rows are plain text", async ({
    page,
  }) => {
    test.skip(!isWide(page), "desktop layout");
    await stubCloud(page, created);
    await page.goto("/setup");
    await expect(page).toHaveURL(/\/setup\/tier$/);
    const rows = nav(page).locator(".rows li");
    await expect(rows).toHaveCount(10);
    await expect(
      nav(page).getByText(t("ui.wizard.step_of", { n: 2, total: 10 }), { exact: true }),
    ).toBeVisible();
    for (let i = 0; i < 10; i++) {
      const box = await rows.nth(i).boundingBox();
      expect(box!.height).toBeGreaterThanOrEqual(44);
    }
    const cafe = nav(page).getByRole("link", { name: t("ui.wizard.step.cafe"), exact: false });
    await expect(cafe).toHaveAttribute("href", "/setup/cafe");
    await expect(cafe).toContainText(t("ui.wizard.done"));
    const current = nav(page).locator('[aria-current="step"]');
    await expect(current).toHaveCount(1);
    await expect(current).toContainText(t("ui.wizard.step.tier"));
    await expect(nav(page).getByRole("link")).toHaveCount(2);
    const devices = rows.nth(3);
    await expect(devices).toContainText(t("ui.wizard.step.devices"));
    await expect(devices.locator("a")).toHaveCount(0);
    await expect(nav(page).getByText(t("ui.wizard.can_skip"), { exact: true })).toHaveCount(3);
    for (const key of ["products", "staff", "printer_payments"] as const) {
      await expect(nav(page).locator(`[data-step="${key}"]`)).toContainText(
        t("ui.wizard.can_skip"),
      );
    }
  });

  test("below 840 px: dots plus a step line", async ({ page }) => {
    test.skip(isWide(page), "phone layout");
    await stubCloud(page, created);
    await page.goto("/setup");
    await expect(page).toHaveURL(/\/setup\/tier$/);
    const line = t("ui.wizard.step_of_named", {
      n: 2,
      total: 10,
      name: t("ui.wizard.step.tier"),
    });
    await expect(nav(page).getByText(line, { exact: true })).toBeVisible();
    await expect(nav(page).locator(".dots")).toHaveAttribute("aria-hidden", "true");
    await expect(nav(page).locator(".dot")).toHaveCount(10);
    await expect(nav(page).getByRole("link")).toHaveCount(0);
    await expect(nav(page).getByRole("listitem")).toHaveCount(0);
  });
});

test.describe("routes", () => {
  test("a set-up owner with no café opens step 1, prefilled from sign-up", async ({ page }) => {
    await stubCloud(page, ready);
    await page.goto("/setup");
    await expect(page).toHaveURL(/\/setup\/cafe$/);
    await expect(h1(page, "ui.wizard.step.cafe")).toBeVisible();
    await expect(field(page, "onboarding.field.cafe_name")).toHaveValue("Pixel Arena Gaming");
    await expect(field(page, "onboarding.field.city")).toHaveValue("Pune");
    await expect(field(page, "onboarding.cafe.field.country")).toHaveValue("IN");
    await expect(field(page, "onboarding.cafe.field.currency")).toHaveValue("INR");
    await expect(field(page, "onboarding.cafe.field.timezone")).toHaveValue("Asia/Kolkata");
    await expect(page.getByTestId("branch-code")).toHaveCount(0);
  });

  test("resume opens the first incomplete step", async ({ page }) => {
    await stubCloud(page, created);
    await page.goto("/setup");
    await expect(page).toHaveURL(/\/setup\/tier$/);
    await expect(h1(page, "ui.wizard.step.tier")).toBeVisible();
    await expect(page.getByTestId("plan-free")).toBeVisible();
  });

  test("a step without its story yet says it is coming soon", async ({ page }) => {
    await stubCloud(page, {
      ...ready,
      wizard: { branch: {}, done: ["cafe", "tier", "install_hub", "devices"] },
    });
    await page.goto("/setup");
    await expect(page).toHaveURL(/\/setup\/rates$/);
    await expect(page.getByText(t("onboarding.setup.coming_soon"))).toBeVisible();
  });

  test("the owner sees the Hub sign-in code once, with Copy, after step 3", async ({ page }) => {
    const cloud = await stubCloud(page, {
      ...ready,
      wizard: { branch: {}, done: ["cafe", "tier", "install_hub"], setupCode: "7K2M9PQR" },
    });
    await page.goto("/setup/devices");
    const text = page.getByTestId("setup-code-text");
    await expect(text).toHaveText(t("onboarding.setup_code.text", { code: "7K2M 9PQR" }));
    await expect(
      page.getByRole("button", { name: t("ui.pairing_code.copy"), exact: true }),
    ).toBeVisible();
    await expect(text).toBeVisible();
    expect(cloud.setupReveals).toBe(1);
    await page.reload();
    await expect(h1(page, "ui.wizard.step.devices")).toBeVisible();
    await expect(page.getByTestId("setup-code-notice")).toHaveCount(0);
    expect(cloud.setupReveals).toBe(2);
  });

  test("a later step's URL redirects to the current step", async ({ page }) => {
    await stubCloud(page, created);
    await page.goto("/setup/devices");
    await expect(page).toHaveURL(/\/setup\/tier$/);
    await expect(h1(page, "ui.wizard.step.tier")).toBeVisible();
    await page.goto("/setup/not-a-step");
    await expect(page).toHaveURL(/\/setup\/tier$/);
  });

  test("an owner who hasn't finished account setup goes back to it", async ({ page }) => {
    await stubCloud(page, { signedIn: true });
    await page.goto("/setup/cafe");
    await expect(page).toHaveURL(/\/account-setup$/);
  });

  test("account setup's done step links to the wizard", async ({ page }) => {
    await stubCloud(page, ready);
    await page.goto("/account-setup");
    await page.getByRole("link", { name: t("onboarding.done.body") }).click();
    await expect(page).toHaveURL(/\/setup\/cafe$/);
  });
});

test.describe("step 1", () => {
  test("country sets currency and time zone, which stay changeable", async ({ page }) => {
    await stubCloud(page, ready);
    await page.goto("/setup/cafe");
    await field(page, "onboarding.cafe.field.country").selectOption("NP");
    await expect(field(page, "onboarding.cafe.field.currency")).toHaveValue("NPR");
    await expect(field(page, "onboarding.cafe.field.timezone")).toHaveValue("Asia/Kathmandu");
    await expect(page.getByRole("group", { name: t("onboarding.cafe.gst.legend") })).toHaveCount(0);
    await field(page, "onboarding.cafe.field.currency").selectOption("USD");
    await field(page, "onboarding.cafe.field.timezone").selectOption("UTC");
    await expect(field(page, "onboarding.cafe.field.currency")).toHaveValue("USD");
    await field(page, "onboarding.cafe.field.country").selectOption("IN");
    await expect(field(page, "onboarding.cafe.field.currency")).toHaveValue("INR");
    await expect(page.getByRole("group", { name: t("onboarding.cafe.gst.legend") })).toBeVisible();
  });

  test("GSTIN shows and is required only with GST, and is checked", async ({ page }) => {
    const cloud = await stubCloud(page, ready);
    await page.goto("/setup/cafe");
    const gstin = field(page, "onboarding.cafe.field.gstin");
    await expect(gstin).toHaveCount(0);
    await field(page, "onboarding.cafe.gst.yes").check();
    await expect(gstin).toBeVisible();
    await button(page, "onboarding.cafe.save").click();
    await expect(page.getByText(t("onboarding.cafe.error.gstin_required"))).toBeVisible();
    await expect(gstin).toBeFocused();
    expect(cloud.cafeSaves).toHaveLength(0);

    await gstin.fill(GSTIN.slice(0, 14) + "A");
    await field(page, "onboarding.field.city").focus();
    await expect(page.getByText(t("onboarding.cafe.error.gstin_invalid"))).toBeVisible();
    await expect(gstin).toHaveAttribute("aria-invalid", "true");

    await field(page, "onboarding.cafe.gst.no").check();
    await expect(gstin).toHaveCount(0);
    await field(page, "onboarding.cafe.gst.yes").check();
    await expect(field(page, "onboarding.cafe.field.gstin")).toHaveValue("");
    await field(page, "onboarding.cafe.field.gstin").fill(GSTIN.toLowerCase());
    await button(page, "onboarding.cafe.save").click();
    await expect(page).toHaveURL(/\/setup\/tier$/);
    expect(cloud.cafeSaves).toHaveLength(1);
    expect(cloud.cafeSaves[0]).toMatchObject({ gst_registered: true, gstin: GSTIN });
  });

  test("without GST no GSTIN is sent", async ({ page }) => {
    const cloud = await stubCloud(page, ready);
    await page.goto("/setup/cafe");
    await field(page, "onboarding.cafe.gst.yes").check();
    await field(page, "onboarding.cafe.field.gstin").fill(GSTIN);
    await field(page, "onboarding.cafe.gst.no").check();
    await button(page, "onboarding.cafe.save").click();
    await expect(page).toHaveURL(/\/setup\/tier$/);
    expect(cloud.cafeSaves[0].gst_registered).toBe(false);
    expect(cloud.cafeSaves[0]).not.toHaveProperty("gstin");
  });

  test("blur and submit check the fields and focus the first invalid one", async ({ page }) => {
    const cloud = await stubCloud(page, ready);
    await page.goto("/setup/cafe");
    await field(page, "onboarding.field.city").fill("x".repeat(81));
    await field(page, "onboarding.field.city").press("Tab");
    await expect(page.getByText(t("onboarding.error.city"))).toBeVisible();
    await field(page, "onboarding.field.cafe_name").fill("");
    await button(page, "onboarding.cafe.save").click();
    await expect(page.getByText(t("onboarding.error.cafe_name"))).toBeVisible();
    await expect(field(page, "onboarding.field.cafe_name")).toBeFocused();
    expect(cloud.cafeSaves).toHaveLength(0);
  });

  test("a server field error lands on its field", async ({ page }) => {
    const cloud = await stubCloud(page, ready);
    await page.goto("/setup/cafe");
    await expect(h1(page, "ui.wizard.step.cafe")).toBeVisible();
    cloud.nextWizard.push({
      status: 422,
      body: { code: "VALIDATION_FAILED", message: "stub", field: "timezone" },
    });
    await button(page, "onboarding.cafe.save").click();
    await expect(page.getByText(t("onboarding.cafe.error.timezone"))).toBeVisible();
    await expect(field(page, "onboarding.cafe.field.timezone")).toBeFocused();
  });

  test("saving announces the next step once and focuses its heading", async ({ page }) => {
    await stubCloud(page, ready);
    await page.goto("/setup/cafe");
    await expect(h1(page, "ui.wizard.step.cafe")).toBeVisible();
    const live = page.getByTestId("wizard-announcement");
    await expect(live).toHaveAttribute("aria-live", "polite");
    await expect(live).toHaveText("");
    await button(page, "onboarding.cafe.save").click();
    await expect(page).toHaveURL(/\/setup\/tier$/);
    await expect(h1(page, "ui.wizard.step.tier")).toBeFocused();
    const said = t("onboarding.setup.announce", {
      n: 2,
      total: 10,
      name: t("ui.wizard.step.tier"),
    });
    await expect(live).toHaveText(said);
    await expect(page.getByText(said, { exact: true })).toHaveCount(1);
  });

  test("a done step opens for edit and saving returns to the current step", async ({ page }) => {
    const cloud = await stubCloud(page, created);
    await page.goto("/setup/tier");
    await expect(h1(page, "ui.wizard.step.tier")).toBeVisible();
    if (isWide(page)) {
      await nav(page)
        .getByRole("link", { name: t("ui.wizard.step.cafe"), exact: false })
        .click();
    } else {
      await page.goto("/setup/cafe");
    }
    await expect(page).toHaveURL(/\/setup\/cafe$/);
    await expect(h1(page, "ui.wizard.step.cafe")).toBeVisible();
    await expect(field(page, "onboarding.field.city")).toHaveValue("Pune");
    await field(page, "onboarding.field.city").fill("Mumbai");
    await button(page, "onboarding.cafe.save").click();
    await expect(page).toHaveURL(/\/setup\/tier$/);
    await expect(h1(page, "ui.wizard.step.tier")).toBeFocused();
    expect(cloud.cafeSaves.at(-1)?.city).toBe("Mumbai");
    expect(cloud.done.has("cafe")).toBe(true);
    await expect(page.getByTestId("wizard-announcement")).toHaveText("");
  });

  test("the branch code can be edited while editable", async ({ page }) => {
    const cloud = await stubCloud(page, created);
    await page.goto("/setup/cafe");
    await expect(page.getByTestId("branch-code")).toHaveText("PAG");
    await button(page, "onboarding.cafe.code.edit").click();
    const input = field(page, "onboarding.cafe.code.field");
    await expect(input).toBeFocused();
    await expect(input).toHaveValue("PAG");

    await input.fill("ab");
    await button(page, "onboarding.cafe.code.save").click();
    await expect(page.getByText(t("onboarding.cafe.code.invalid"))).toBeVisible();
    expect(cloud.codeSets).toHaveLength(0);

    await input.fill("sec");
    await button(page, "onboarding.cafe.code.save").click();
    await expect(page.getByText(t("onboarding.cafe.code.taken"))).toBeVisible();
    await expect(input).toBeFocused();

    await input.fill("arena1");
    await button(page, "onboarding.cafe.code.save").click();
    await expect(page.getByTestId("branch-code")).toHaveText("ARENA1");
    await expect(page.getByRole("status").getByText(t("onboarding.cafe.code.saved"))).toBeVisible();
    await expect(button(page, "onboarding.cafe.code.edit")).toBeFocused();
    expect(cloud.codeSets.at(-1)).toEqual({ id: cloud.branch!.id, code: "ARENA1" });
    await expect(page).toHaveURL(/\/setup\/cafe$/);
  });

  test("a locked branch code has no edit control", async ({ page }) => {
    await stubCloud(page, { ...ready, wizard: { branch: { code_editable: false } } });
    await page.goto("/setup/cafe");
    await expect(page.getByTestId("branch-code")).toHaveText("PAG");
    await expect(page.getByText(t("onboarding.cafe.code.locked_hint"))).toBeVisible();
    await expect(button(page, "onboarding.cafe.code.edit")).toHaveCount(0);
  });
});

async function clientNav(page: Page, href: string) {
  await page.evaluate((to) => {
    const a = document.createElement("a");
    a.href = to;
    document.body.append(a);
    a.click();
    a.remove();
  }, href);
}

test.describe("fresh state", () => {
  test("a done step after the current one opens; a pending one redirects", async ({ page }) => {
    await stubCloud(page, { ...ready, wizard: { branch: {}, done: ["cafe", "install_hub"] } });
    await page.goto("/setup/install_hub");
    await expect(h1(page, "ui.wizard.step.install_hub")).toBeVisible();
    await expect(page).toHaveURL(/\/setup\/install_hub$/);
    await page.goto("/setup/devices");
    await expect(page).toHaveURL(/\/setup\/tier$/);
  });

  test("after signing out, the next owner sees only their own setup", async ({ page }) => {
    const cloud = await stubCloud(page, created);
    await page.goto("/account-setup");
    await page.getByRole("link", { name: t("onboarding.done.body") }).click();
    await expect(page).toHaveURL(/\/setup\/tier$/);
    await clientNav(page, "/setup/cafe");
    await expect(page.getByTestId("branch-code")).toHaveText("PAG");

    await clientNav(page, "/account/security");
    await button(page, "onboarding.security.signout_button").click();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: t("onboarding.security.signout_button"), exact: true })
      .click();
    await expect(page).toHaveURL(/\/sign-in$/);

    cloud.branch = null;
    cloud.tenant = null;
    cloud.done.clear();
    cloud.owner.cafe_name = "Level Up";
    cloud.trusted = true;
    await field(page, "onboarding.signin.login").fill("+919876543211");
    await field(page, "onboarding.password.label").fill(PASSWORD);
    await button(page, "onboarding.signin.submit").click();
    await expect(page).toHaveURL(/\/account-setup$/);
    await page.getByRole("link", { name: t("onboarding.done.body") }).click();
    await expect(page).toHaveURL(/\/setup\/cafe$/);
    await expect(field(page, "onboarding.field.cafe_name")).toHaveValue("Level Up");
    await expect(page.getByTestId("branch-code")).toHaveCount(0);
  });

  test("returning to the tab reloads progress made elsewhere", async ({ page }) => {
    const cloud = await stubCloud(page, created);
    await page.goto("/setup");
    await expect(page).toHaveURL(/\/setup\/tier$/);
    cloud.done.add("tier");
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await expect(page).toHaveURL(/\/setup\/tier$/);
    await expect(nav(page).locator('[data-step="install_hub"]')).toHaveAttribute(
      "data-state",
      "pending",
    );
    await expect(nav(page).locator('[data-step="tier"]')).toHaveAttribute("data-state", "done");
  });
});

test.describe("failures", () => {
  test("a locked code reply with a failed refresh hides Edit and keeps the code", async ({
    page,
  }) => {
    const cloud = await stubCloud(page, created);
    await page.goto("/setup/cafe");
    await expect(page.getByTestId("branch-code")).toHaveText("PAG");
    await button(page, "onboarding.cafe.code.edit").click();
    await field(page, "onboarding.cafe.code.field").fill("NEW1");
    cloud.nextWizard.push(
      { status: 409, body: err("ALREADY_SET") },
      { status: 500, body: err("INTERNAL") },
    );
    await button(page, "onboarding.cafe.code.save").click();
    await expect(page.getByText(t("onboarding.cafe.code.locked"), { exact: true })).toBeVisible();
    await expect(
      page.getByText(t("onboarding.cafe.code.locked_hint"), { exact: true }),
    ).toBeVisible();
    await expect(button(page, "onboarding.cafe.code.edit")).toHaveCount(0);
    await expect(page.getByTestId("branch-code")).toHaveText("PAG");
  });

  test("a failed wizard load shows a retry that recovers", async ({ page }) => {
    const cloud = await stubCloud(page, created);
    cloud.nextWizard.push({ status: 500, body: err("INTERNAL") });
    await page.goto("/setup");
    await expect(page.getByRole("alert")).toHaveText(t("onboarding.error.generic"));
    await button(page, "onboarding.account_setup.retry").click();
    await expect(page).toHaveURL(/\/setup\/tier$/);
    await expect(h1(page, "ui.wizard.step.tier")).toBeVisible();
  });

  test("offline then back shows a retry that recovers", async ({ page }) => {
    const cloud = await stubCloud(page, created);
    cloud.nextWizard.push({ status: 0, body: null, abort: true });
    await page.goto("/setup");
    await expect(page.getByRole("alert")).toHaveText(t("onboarding.error.offline"));
    await button(page, "onboarding.account_setup.retry").click();
    await expect(h1(page, "ui.wizard.step.tier")).toBeVisible();
  });

  test("a signed-out session lands on the start page", async ({ page }) => {
    await stubCloud(page, { owner: READY_OWNER });
    await page.goto("/setup/cafe");
    await expect(page).toHaveURL(/\/$/);
  });

  test("a failed sign-up prefill shows a retry", async ({ page }) => {
    const cloud = await stubCloud(page, ready);
    cloud.nextSession.push({ status: 500, body: err("INTERNAL") });
    await page.goto("/setup/cafe");
    await expect(page.getByRole("alert")).toContainText(t("onboarding.error.generic"));
    await button(page, "onboarding.account_setup.retry").click();
    await expect(field(page, "onboarding.field.cafe_name")).toHaveValue("Pixel Arena Gaming");
  });

  test("saving after account setup was reset goes back to it", async ({ page }) => {
    const cloud = await stubCloud(page, ready);
    await page.goto("/setup/cafe");
    await expect(field(page, "onboarding.field.cafe_name")).toHaveValue("Pixel Arena Gaming");
    cloud.nextWizard.push({ status: 403, body: err("SETUP_INCOMPLETE") });
    await button(page, "onboarding.cafe.save").click();
    await expect(page).toHaveURL(/\/account-setup$/);
  });
});
