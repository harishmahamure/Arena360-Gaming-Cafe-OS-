import { expect, test, type Page } from "@playwright/test";
import { CHALLENGE, GOOD_CODE, err, fillForm, stubCloud, t, toCodeStep } from "./stub";

// Sign-up with a phone code: the form's validation and focus, the code step (paste, a wrong
// code, the expired and rate-limited states, the resend countdown) and landing on
// `/account-setup`. The cloud is stubbed with `page.route`.

const field = (page: Page, key: Parameters<typeof t>[0]) =>
  page.getByLabel(t(key), { exact: true });
const codeInput = (page: Page) => field(page, "onboarding.code.label");
const cells = (page: Page) => page.locator('[data-component="code-field"] .cell');
const resendButton = (page: Page) =>
  page.getByRole("button", { name: t("onboarding.code.resend") });

test.describe("Get started form", () => {
  test("an empty form sends nothing, marks every field and focuses the first", async ({ page }) => {
    const cloud = await stubCloud(page);
    await page.goto("/");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("onboarding.start.title"));
    await expect(field(page, "onboarding.field.country")).toHaveValue("+91");
    await page.getByRole("button", { name: t("onboarding.start.send") }).click();
    await expect(field(page, "onboarding.field.name")).toBeFocused();
    for (const [key, message] of [
      ["onboarding.field.name", "onboarding.error.name"],
      ["onboarding.field.phone", "onboarding.error.phone_in"],
      ["onboarding.field.email", "onboarding.error.email"],
      ["onboarding.field.cafe_name", "onboarding.error.cafe_name"],
      ["onboarding.field.city", "onboarding.error.city"],
    ] as const) {
      await expect(field(page, key)).toHaveAttribute("aria-invalid", "true");
      await expect(page.getByText(t(message), { exact: true })).toBeVisible();
    }
    expect(cloud.starts).toEqual([]);
  });

  test("an Indian number needs 10 digits from 6–9; focus goes to the phone", async ({ page }) => {
    const cloud = await stubCloud(page);
    await page.goto("/");
    await fillForm(page);
    await field(page, "onboarding.field.phone").fill("12345");
    await page.getByRole("button", { name: t("onboarding.start.send") }).click();
    await expect(field(page, "onboarding.field.phone")).toBeFocused();
    await expect(page.getByText(t("onboarding.error.phone_in"))).toBeVisible();
    expect(cloud.starts).toEqual([]);
    // Another country takes 8–15 digits in all.
    await field(page, "onboarding.field.country").selectOption("+44");
    await field(page, "onboarding.field.phone").fill("7700 900123");
    await page.getByRole("button", { name: t("onboarding.start.send") }).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("onboarding.code.title"));
    expect(cloud.starts).toEqual([
      {
        name: "Asha Rao",
        phone: "+447700900123",
        email: "asha@example.in",
        cafe_name: "Pixel Den",
        city: "Pune",
      },
    ]);
  });

  test("a field is checked when it loses focus", async ({ page }) => {
    await stubCloud(page);
    await page.goto("/");
    await field(page, "onboarding.field.email").fill("asha@");
    await page.keyboard.press("Tab");
    await expect(field(page, "onboarding.field.email")).toHaveAttribute("aria-invalid", "true");
    await expect(page.getByText(t("onboarding.error.email"))).toBeVisible();
    await field(page, "onboarding.field.email").fill("asha@example.in");
    await page.keyboard.press("Tab");
    await expect(field(page, "onboarding.field.email")).not.toHaveAttribute("aria-invalid");
  });

  test("the cloud's VALIDATION_FAILED field is marked and focused", async ({ page }) => {
    const cloud = await stubCloud(page);
    cloud.nextStart.push({ status: 422, body: err("VALIDATION_FAILED", { field: "city" }) });
    await page.goto("/");
    await fillForm(page);
    await page.getByRole("button", { name: t("onboarding.start.send") }).click();
    await expect(field(page, "onboarding.field.city")).toBeFocused();
    await expect(field(page, "onboarding.field.city")).toHaveAttribute("aria-invalid", "true");
  });

  test("RATE_LIMITED says when to try again from Retry-After", async ({ page }) => {
    const cloud = await stubCloud(page);
    cloud.nextStart.push({
      status: 429,
      body: err("RATE_LIMITED"),
      headers: { "retry-after": "125" },
    });
    await page.goto("/");
    await fillForm(page);
    await page.getByRole("button", { name: t("onboarding.start.send") }).click();
    await expect(page.getByRole("alert")).toContainText(
      t("onboarding.error.rate_limited", { time: "2:05" }),
    );
  });
});

test.describe("code step", () => {
  test("a pasted code fills every cell, submits and lands on account setup", async ({ page }) => {
    const cloud = await stubCloud(page);
    await toCodeStep(page);
    await expect(
      page.getByText(t("onboarding.code.sent", { phone: "+919876543210" })),
    ).toBeVisible();
    await expect(codeInput(page)).toBeFocused();
    await expect(codeInput(page)).toHaveAttribute("autocomplete", "one-time-code");
    await expect(codeInput(page)).toHaveAttribute("inputmode", "numeric");
    await expect(cells(page)).toHaveCount(6);
    await page.keyboard.insertText("123 456");
    await expect(page).toHaveURL(/\/account-setup$/);
    expect(cloud.verifies).toEqual([{ challenge_id: CHALLENGE.challenge_id, code: GOOD_CODE }]);
    await expect(
      page.getByRole("heading", { level: 1, name: t("onboarding.password.title") }),
    ).toBeVisible();
  });

  test("a wrong code clears the cells and says so", async ({ page }) => {
    const cloud = await stubCloud(page);
    await toCodeStep(page);
    await page.keyboard.insertText("654321");
    await expect(page.getByText(t("onboarding.code.invalid"))).toBeVisible();
    await expect(codeInput(page)).toHaveValue("");
    await expect(codeInput(page)).toHaveAttribute("aria-invalid", "true");
    await expect(codeInput(page)).toBeFocused();
    for (let i = 0; i < 6; i++) await expect(cells(page).nth(i)).toHaveText("");
    // Typing digit by digit submits once the sixth is in.
    for (const d of GOOD_CODE) await page.keyboard.type(d);
    await expect(page).toHaveURL(/\/account-setup$/);
    expect(cloud.verifies.map((v) => v.code)).toEqual(["654321", GOOD_CODE]);
  });

  test("an expired code offers a new one at once", async ({ page }) => {
    const cloud = await stubCloud(page);
    cloud.nextVerify.push({ status: 400, body: err("OTP_EXPIRED") });
    await toCodeStep(page);
    await expect(resendButton(page)).toHaveAttribute("aria-disabled", "true");
    await page.keyboard.insertText(GOOD_CODE);
    await expect(page.getByText(t("onboarding.code.expired"))).toBeVisible();
    await expect(resendButton(page)).not.toHaveAttribute("aria-disabled");
    await resendButton(page).click();
    await expect(page.getByText(t("onboarding.code.resent"))).toBeVisible();
    expect(cloud.resends).toEqual([{ challenge_id: CHALLENGE.challenge_id }]);
  });

  test("Resend code waits out the countdown, announced at 30 s and at 0", async ({ page }) => {
    await page.clock.install();
    const cloud = await stubCloud(page);
    await toCodeStep(page);
    // Stop the clock so the countdown moves only with `runFor`.
    await page.clock.pauseAt((await page.evaluate(() => Date.now())) + 100);
    const announcement = page.getByTestId("resend-announcement");
    await expect(resendButton(page)).toHaveAttribute("aria-disabled", "true");
    await expect(page.getByTestId("resend-countdown")).toHaveText(
      t("onboarding.code.resend_in", { time: "0:30" }),
    );
    await expect(announcement).toHaveText(t("onboarding.code.resend_in", { time: "0:30" }));
    await resendButton(page).click({ force: true });
    expect(cloud.resends).toEqual([]);
    await page.clock.runFor(10_000);
    await expect(page.getByTestId("resend-countdown")).toHaveText(
      t("onboarding.code.resend_in", { time: "0:20" }),
    );
    // Not re-announced on every tick.
    await expect(announcement).toHaveText(t("onboarding.code.resend_in", { time: "0:30" }));
    await page.clock.runFor(21_000);
    await expect(announcement).toHaveText(t("onboarding.code.resend_ready"));
    await expect(page.getByTestId("resend-countdown")).toHaveCount(0);
    await expect(resendButton(page)).not.toHaveAttribute("aria-disabled");
    await resendButton(page).click();
    await expect(page.getByText(t("onboarding.code.resent"))).toBeVisible();
    expect(cloud.resends).toHaveLength(1);
    await expect(resendButton(page)).toHaveAttribute("aria-disabled", "true");
  });

  test("a rate-limited resend says when to try again", async ({ page }) => {
    await page.clock.install();
    const cloud = await stubCloud(page);
    cloud.nextResend.push({
      status: 429,
      body: err("RATE_LIMITED"),
      headers: { "retry-after": "1800" },
    });
    await toCodeStep(page);
    await page.clock.runFor(31_000);
    await resendButton(page).click();
    await expect(
      page.getByText(t("onboarding.error.rate_limited", { time: "30:00" })),
    ).toBeVisible();
    await expect(resendButton(page)).toHaveAttribute("aria-disabled", "true");
  });

  test("an owner who already enrolled is told to sign in", async ({ page }) => {
    const cloud = await stubCloud(page);
    cloud.nextVerify.push({ status: 200, body: { next: "sign_in" } });
    await toCodeStep(page);
    await page.keyboard.insertText(GOOD_CODE);
    await expect(page.getByText(t("onboarding.sign_in.message"))).toBeVisible();
    await expect(page.getByRole("link", { name: t("onboarding.sign_in.link") })).toHaveAttribute(
      "href",
      "/sign-in",
    );
    await expect(page).toHaveURL(/\/$/);
  });
});

test.describe("code step: dead ends and late answers", () => {
  test("a challenge that can't be resent offers a new code or new details", async ({ page }) => {
    await page.clock.install();
    const cloud = await stubCloud(page);
    const second = { ...CHALLENGE, challenge_id: "01JCHALLENGE00000000000002" };
    cloud.nextResend.push({ status: 400, body: err("OTP_EXPIRED") });
    await toCodeStep(page);
    await page.clock.runFor(31_000);
    await resendButton(page).click();
    await expect(page.getByText(t("onboarding.code.expired"))).toBeVisible();
    await expect(resendButton(page)).toHaveCount(0);
    await expect(page.getByRole("button", { name: t("onboarding.code.change") })).toBeVisible();
    cloud.nextStart.push({ status: 202, body: second });
    await page.getByRole("button", { name: t("onboarding.code.send_new") }).click();
    await expect(resendButton(page)).toBeVisible();
    await expect(page.getByText(t("onboarding.code.expired"))).toHaveCount(0);
    expect(cloud.starts).toHaveLength(2);
    expect(cloud.starts[1]).toEqual(cloud.starts[0]);
    await page.keyboard.insertText(GOOD_CODE);
    await expect(page).toHaveURL(/\/account-setup$/);
    expect(cloud.verifies.at(-1)).toEqual({ challenge_id: second.challenge_id, code: GOOD_CODE });
  });

  test("a verify answered after Change details does not revive the old challenge", async ({
    page,
  }) => {
    const cloud = await stubCloud(page);
    cloud.nextVerify.push({ status: 200, body: { next: "account_setup" }, delayMs: 1500 });
    await toCodeStep(page);
    await page.keyboard.insertText(GOOD_CODE);
    await page.getByRole("button", { name: t("onboarding.code.change") }).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("onboarding.start.title"));
    await page.waitForTimeout(2000);
    expect(cloud.verifies).toHaveLength(1);
    await expect(page).toHaveURL(/\/$/);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("onboarding.start.title"));
  });

  test("a resend answered after a new start is dropped", async ({ page }) => {
    await page.clock.install();
    const cloud = await stubCloud(page);
    const second = { ...CHALLENGE, challenge_id: "01JCHALLENGE00000000000002" };
    await toCodeStep(page);
    await page.clock.runFor(31_000);
    cloud.nextResend.push({ status: 400, body: err("OTP_EXPIRED"), delayMs: 1500 });
    await resendButton(page).click();
    await page.getByRole("button", { name: t("onboarding.code.change") }).click();
    cloud.nextStart.push({ status: 202, body: second });
    await page.getByRole("button", { name: t("onboarding.start.send") }).click();
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("onboarding.code.title"));
    await page.waitForTimeout(2000);
    expect(cloud.resends).toHaveLength(1);
    await expect(page.getByText(t("onboarding.code.expired"))).toHaveCount(0);
    await expect(resendButton(page)).toBeVisible();
    await page.keyboard.insertText(GOOD_CODE);
    await expect(page).toHaveURL(/\/account-setup$/);
    expect(cloud.verifies.at(-1)?.challenge_id).toBe(second.challenge_id);
  });
});

test.describe("account setup", () => {
  test("without a session it goes back to the start", async ({ page }) => {
    const cloud = await stubCloud(page);
    await page.goto("/account-setup");
    await expect(page).toHaveURL(/\/$/);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("onboarding.start.title"));
    // The refresh was tried once first.
    expect(cloud.refreshes).toBe(1);
  });

  test("an expired access cookie is refreshed and the owner stays", async ({ page }) => {
    const cloud = await stubCloud(page);
    cloud.signedIn = true;
    cloud.nextSession.push({ status: 401, body: err("AUTH_FAILED") });
    await page.goto("/account-setup");
    await expect(
      page.getByRole("heading", { level: 1, name: t("onboarding.password.title") }),
    ).toBeVisible();
    await expect(page).toHaveURL(/\/account-setup$/);
    expect(cloud.refreshes).toBe(1);
    expect(cloud.sessionChecks).toBe(2);
  });

  test("a refused refresh goes back to the start", async ({ page }) => {
    const cloud = await stubCloud(page);
    cloud.signedIn = true;
    cloud.nextSession.push({ status: 401, body: err("AUTH_FAILED") });
    cloud.nextRefresh.push({ status: 401, body: err("AUTH_FAILED") });
    await page.goto("/account-setup");
    await expect(page).toHaveURL(/\/$/);
    expect(cloud.sessionChecks).toBe(1);
  });

  test("offline shows a retry, and the retry signs in", async ({ page }) => {
    const cloud = await stubCloud(page);
    cloud.signedIn = true;
    cloud.nextSession.push({ status: 0, body: null, abort: true });
    await page.goto("/account-setup");
    await expect(page.getByText(t("onboarding.error.offline"))).toBeVisible();
    await expect(page).toHaveURL(/\/account-setup$/);
    await page.getByRole("button", { name: t("onboarding.account_setup.retry") }).click();
    await expect(
      page.getByRole("heading", { level: 1, name: t("onboarding.password.title") }),
    ).toBeVisible();
    expect(cloud.refreshes).toBe(0);
  });

  test("a server error shows a retry instead of signing out", async ({ page }) => {
    const cloud = await stubCloud(page);
    cloud.signedIn = true;
    cloud.nextSession.push({ status: 500, body: err("INTERNAL") });
    await page.goto("/account-setup");
    await expect(page.getByText(t("onboarding.error.generic"))).toBeVisible();
    await expect(page).toHaveURL(/\/account-setup$/);
  });
});
