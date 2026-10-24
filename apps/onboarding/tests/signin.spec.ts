import { expect, test, type Page } from "@playwright/test";
import {
  ENROLLED_OWNER,
  GOOD_CODE,
  GOOD_TOTP,
  PASSWORD,
  SIGNIN_CHALLENGE,
  err,
  stubCloud,
  t,
} from "./stub";

type Key = Parameters<typeof t>[0];
const h1 = (page: Page, key: Key, params?: Record<string, string | number>) =>
  page.getByRole("heading", { level: 1, name: t(key, params) });
const button = (page: Page, key: Key) => page.getByRole("button", { name: t(key), exact: true });
const loginInput = (page: Page) => page.getByLabel(t("onboarding.signin.login"), { exact: true });
const passwordInput = (page: Page) =>
  page.getByLabel(t("onboarding.password.label"), { exact: true });
const totpInput = (page: Page) => page.getByLabel(t("onboarding.totp.code_label"), { exact: true });

async function fillSignin(page: Page, password = PASSWORD) {
  await page.goto("/sign-in");
  await loginInput(page).fill("98765 43210");
  await passwordInput(page).fill(password);
  await button(page, "onboarding.signin.submit").click();
}

async function toChallenge(page: Page) {
  await fillSignin(page);
  await expect(h1(page, "onboarding.signin.challenge_title")).toBeVisible();
}

test.describe("sign-in form", () => {
  test("the fields are labelled for password managers and checked before sending", async ({
    page,
  }) => {
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    await page.goto("/sign-in");
    await expect(h1(page, "onboarding.signin.title")).toBeVisible();
    await expect(loginInput(page)).toHaveAttribute("autocomplete", "username");
    await expect(passwordInput(page)).toHaveAttribute("autocomplete", "current-password");
    await button(page, "onboarding.signin.submit").click();
    await expect(page.getByText(t("onboarding.signin.login_required"))).toBeVisible();
    await expect(page.getByText(t("onboarding.signin.password_required"))).toBeVisible();
    await expect(loginInput(page)).toBeFocused();
    expect(cloud.signins).toEqual([]);
    await expect(
      page.getByRole("link", { name: t("onboarding.signin.get_started") }),
    ).toHaveAttribute("href", "/");
  });

  test("a wrong password shows one plain message", async ({ page }) => {
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    await fillSignin(page, "Not-The-Password");
    await expect(
      page.getByRole("alert").filter({ hasText: t("onboarding.signin.failed") }),
    ).toBeVisible();
    expect(cloud.signins).toEqual([{ login: "98765 43210", password: "Not-The-Password" }]);
    await expect(h1(page, "onboarding.signin.title")).toBeVisible();
  });

  test("a paused account says when to try again", async ({ page }) => {
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    cloud.nextSignin.push({
      status: 429,
      body: err("AUTH_LOCKED"),
      headers: { "retry-after": "754" },
    });
    await fillSignin(page);
    const alert = page
      .getByRole("alert")
      .filter({ hasText: t("onboarding.signin.locked", { time: "12:34" }) });
    await expect(alert).toHaveCount(1);
  });

  test("an owner still in setup goes on with account setup", async ({ page }) => {
    await stubCloud(page, { owner: { has_password: true } });
    await fillSignin(page);
    await expect(h1(page, "onboarding.totp.title")).toBeVisible();
    await expect(page).toHaveURL(/\/account-setup$/);
  });
});

test.describe("second factor", () => {
  test("TOTP is the default; a wrong code clears the cells", async ({ page }) => {
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    await toChallenge(page);
    await expect(totpInput(page)).toBeFocused();
    await expect(button(page, "onboarding.signin.text_me")).toHaveCount(0);
    await page.keyboard.insertText("111111");
    await expect(page.getByText(t("onboarding.signin.failed"))).toBeVisible();
    await expect(totpInput(page)).toHaveValue("");
    await expect(totpInput(page)).toBeFocused();
    await page.keyboard.insertText(GOOD_TOTP);
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
    expect(cloud.signinVerifies).toEqual([
      { challenge_id: SIGNIN_CHALLENGE, method: "totp", code: "111111", trust_device: false },
      { challenge_id: SIGNIN_CHALLENGE, method: "totp", code: GOOD_TOTP, trust_device: false },
    ]);
  });

  test("a trusted device skips the code next time", async ({ page }) => {
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    await toChallenge(page);
    const trust = page.getByLabel(t("onboarding.signin.trust"));
    await expect(trust).toHaveAccessibleDescription(t("onboarding.signin.trust_hint"));
    await trust.check();
    await totpInput(page).focus();
    await page.keyboard.insertText(GOOD_TOTP);
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
    expect(cloud.signinVerifies[0].trust_device).toBe(true);

    await fillSignin(page);
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
    expect(cloud.signinVerifies).toHaveLength(1);
  });

  test("a backup code is normalised before it is sent", async ({ page }) => {
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    await toChallenge(page);
    await button(page, "onboarding.signin.use_backup").click();
    const field = page.getByLabel(t("onboarding.signin.backup_label"), { exact: true });
    await expect(field).toBeFocused();
    await field.fill("abcde");
    await button(page, "onboarding.signin.submit").click();
    await expect(page.getByText(t("onboarding.signin.backup_required"))).toBeVisible();
    expect(cloud.signinVerifies).toEqual([]);
    await field.fill("abcde fgoo1");
    await button(page, "onboarding.signin.submit").click();
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
    expect(cloud.signinVerifies).toEqual([
      {
        challenge_id: SIGNIN_CHALLENGE,
        method: "backup_code",
        code: "ABCDEFG001",
        trust_device: false,
      },
    ]);
  });

  test("with 3 or fewer backup codes left it offers to regenerate", async ({ page }) => {
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    cloud.backupRemaining = 2;
    await toChallenge(page);
    await button(page, "onboarding.signin.use_backup").click();
    await page.getByLabel(t("onboarding.signin.backup_label"), { exact: true }).fill("ABCDE-FG000");
    await button(page, "onboarding.signin.submit").click();
    await expect(h1(page, "onboarding.signin.low_title", { n: 2 })).toBeFocused();
    await button(page, "onboarding.signin.regenerate_now").click();
    await expect(h1(page, "onboarding.security.title")).toBeVisible();

    cloud.backupRemaining = 1;
    await toChallenge(page);
    await button(page, "onboarding.signin.use_backup").click();
    await page.getByLabel(t("onboarding.signin.backup_label"), { exact: true }).fill("ABCDE-FG001");
    await button(page, "onboarding.signin.submit").click();
    await expect(h1(page, "onboarding.signin.low_title_one")).toBeVisible();
    await button(page, "onboarding.signin.later").click();
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
  });

  test("text me a code appears only when SMS is on, then counts down to resend", async ({
    page,
  }) => {
    await page.clock.install();
    const cloud = await stubCloud(page, {
      owner: { ...ENROLLED_OWNER, sms_fallback: true },
    });
    await toChallenge(page);
    await button(page, "onboarding.signin.text_me").click();
    await expect(page.getByLabel(t("onboarding.signin.sms_label"), { exact: true })).toBeFocused();
    expect(cloud.smsSends).toEqual([{ challenge_id: SIGNIN_CHALLENGE }]);
    const resend = button(page, "onboarding.code.resend");
    await expect(resend).toHaveAttribute("aria-disabled", "true");
    await expect(page.getByTestId("resend-announcement")).toHaveText(
      t("onboarding.code.resend_in", { time: "0:30" }),
    );
    await page.clock.runFor(31_000);
    await expect(resend).not.toHaveAttribute("aria-disabled", "true");
    await resend.click();
    await expect(page.getByText(t("onboarding.code.resent"))).toBeVisible();
    expect(cloud.smsSends).toHaveLength(2);
    await page.getByLabel(t("onboarding.signin.sms_label"), { exact: true }).focus();
    await page.keyboard.insertText(GOOD_CODE);
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
    expect(cloud.signinVerifies.at(-1)?.method).toBe("sms");
  });

  test("a pause during the code step is shown with its time", async ({ page }) => {
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    cloud.nextSigninVerify.push({
      status: 429,
      body: err("AUTH_LOCKED"),
      headers: { "retry-after": "900" },
    });
    await toChallenge(page);
    await page.keyboard.insertText(GOOD_TOTP);
    await expect(
      page.getByRole("alert").filter({ hasText: t("onboarding.signin.locked", { time: "15:00" }) }),
    ).toBeVisible();
    await button(page, "onboarding.signin.start_over").click();
    await expect(h1(page, "onboarding.signin.title")).toBeFocused();
  });
});

test.describe("account security", () => {
  test("the SMS switch, the trusted devices and sign out everywhere", async ({ page }) => {
    const cloud = await stubCloud(page, { signedIn: true, owner: ENROLLED_OWNER });
    await page.goto("/account/security");
    const sms = page.getByRole("switch", { name: t("onboarding.security.sms_label") });
    await expect(sms).toHaveAttribute("aria-checked", "false");
    await sms.click();
    await expect(sms).toHaveAttribute("aria-checked", "true");
    await expect(page.getByText(t("onboarding.security.sms_on"))).toBeVisible();
    expect(cloud.smsFallbackSets).toEqual([true]);

    const rows = page.getByRole("listitem");
    await expect(rows).toHaveCount(2);
    await expect(rows.first()).toContainText("Chrome on Android");
    await expect(rows.first()).toContainText(t("onboarding.security.this_device"));
    const seen = await page.evaluate(() =>
      new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(
        Date.UTC(2026, 9, 20, 9, 30),
      ),
    );
    await expect(rows.first()).toContainText(t("onboarding.security.last_seen", { time: seen }));
    await rows
      .nth(1)
      .getByRole("button", { name: t("onboarding.security.remove") })
      .click();
    await expect(rows).toHaveCount(1);
    await expect(page.getByText(t("onboarding.security.removed"))).toBeVisible();
    expect(cloud.removedDevices).toEqual(["dev-2"]);

    const open = button(page, "onboarding.security.signout_button");
    await open.click();
    const dialog = page.getByRole("dialog", {
      name: t("onboarding.security.signout_confirm_title"),
    });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: t("onboarding.security.cancel") }).click();
    await expect(dialog).toBeHidden();
    await expect(open).toBeFocused();
    expect(cloud.signOuts).toBe(0);
    await open.click();
    await dialog.getByRole("button", { name: t("onboarding.security.signout_button") }).click();
    await expect(page).toHaveURL(/\/sign-in$/);
    expect(cloud.signOuts).toBe(1);
  });

  test("trust, then sign out everywhere, then the code is asked again", async ({ page }) => {
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    await toChallenge(page);
    await page.getByLabel(t("onboarding.signin.trust")).check();
    await totpInput(page).focus();
    await page.keyboard.insertText(GOOD_TOTP);
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
    await fillSignin(page);
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
    await page.goto("/account/security");
    await button(page, "onboarding.security.signout_button").click();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: t("onboarding.security.signout_button") })
      .click();
    await expect(h1(page, "onboarding.signin.title")).toBeVisible();
    await fillSignin(page);
    await expect(h1(page, "onboarding.signin.challenge_title")).toBeVisible();
    expect(cloud.signinVerifies).toHaveLength(1);
  });
});

test.describe("dead challenges and waits", () => {
  test("five wrong codes end the challenge and offer Sign in again", async ({ page }) => {
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    await toChallenge(page);
    for (let i = 0; i < 4; i++) {
      await totpInput(page).focus();
      await page.keyboard.insertText("111111");
      await expect(page.getByText(t("onboarding.signin.failed"))).toBeVisible();
    }
    await totpInput(page).focus();
    await page.keyboard.insertText("111111");
    await expect(
      page.getByRole("alert").filter({ hasText: t("onboarding.signin.dead") }),
    ).toBeVisible();
    await expect(totpInput(page)).toHaveCount(0);
    expect(cloud.signinVerifies).toHaveLength(5);
    await button(page, "onboarding.signin.again").click();
    await expect(h1(page, "onboarding.signin.title")).toBeFocused();
  });

  test("after five minutes the challenge ends", async ({ page }) => {
    await page.clock.install();
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    await toChallenge(page);
    await page.clock.runFor(5 * 60_000 + 1_000);
    await expect(
      page.getByRole("alert").filter({ hasText: t("onboarding.signin.dead") }),
    ).toBeVisible();
    expect(cloud.signinVerifies).toEqual([]);
    await button(page, "onboarding.signin.again").click();
    await expect(loginInput(page)).toBeVisible();
  });

  test("Text me a code during the wait says how long to wait", async ({ page }) => {
    await page.clock.install();
    const cloud = await stubCloud(page, {
      owner: { ...ENROLLED_OWNER, sms_fallback: true },
    });
    await toChallenge(page);
    await button(page, "onboarding.signin.text_me").click();
    await expect(page.getByLabel(t("onboarding.signin.sms_label"), { exact: true })).toBeVisible();
    await button(page, "onboarding.signin.use_totp").click();
    await page.clock.runFor(10_000);
    await button(page, "onboarding.signin.text_me").click();
    await expect(
      page.getByRole("alert").filter({ hasText: t("onboarding.code.resend_in", { time: "0:20" }) }),
    ).toBeVisible();
    expect(cloud.smsSends).toHaveLength(1);
  });

  test("a reply with neither next nor a challenge is a generic error", async ({ page }) => {
    const cloud = await stubCloud(page, { owner: ENROLLED_OWNER });
    cloud.nextSignin.push({ status: 200, body: {} });
    await fillSignin(page);
    await expect(
      page.getByRole("alert").filter({ hasText: t("onboarding.error.generic") }),
    ).toBeVisible();
    await expect(h1(page, "onboarding.signin.title")).toBeVisible();
  });

  test("removing this device signs out; Remove can't be pressed twice", async ({ page }) => {
    const cloud = await stubCloud(page, { signedIn: true, owner: ENROLLED_OWNER });
    await page.route("**/v1/account/devices/dev-1", async (route) => {
      await new Promise((done) => setTimeout(done, 300));
      await route.fallback();
    });
    await page.goto("/account/security");
    const rows = page.getByRole("listitem");
    await expect(rows).toHaveCount(2);
    const remove = rows.first().getByRole("button", { name: t("onboarding.security.remove") });
    await remove.click();
    await remove.click({ force: true });
    await expect(page).toHaveURL(/\/sign-in$/);
    expect(cloud.removedDevices).toEqual(["dev-1"]);
  });
});
