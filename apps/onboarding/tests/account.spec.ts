import { expect, test, type Page } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { CODES, GOOD_TOTP, NEW_CODES, SECRETS, err, stubCloud, t, toCodeStep } from "./stub";

// Account setup after sign-up (password, authenticator app, backup codes), resuming it, and
// Account → Security's "Regenerate backup codes". The cloud is stubbed with `page.route`.

type Key = Parameters<typeof t>[0];
const h1 = (page: Page, key: Key) => page.getByRole("heading", { level: 1, name: t(key) });
const button = (page: Page, key: Key) => page.getByRole("button", { name: t(key), exact: true });
const passwordInput = (page: Page) =>
  page.getByLabel(t("onboarding.password.label"), { exact: true });
const totpInput = (page: Page) => page.getByLabel(t("onboarding.totp.code_label"), { exact: true });
const cells = (page: Page) => page.locator('[data-component="code-field"] .cell');
const grouped = (s: string) => s.match(/.{1,4}/g)!.join(" ");

test.use({ permissions: ["clipboard-read", "clipboard-write"] });

/** Signs up through the code step, landing on the password step. */
async function signUp(page: Page) {
  await toCodeStep(page);
  await page.keyboard.insertText("123456");
  await expect(h1(page, "onboarding.password.title")).toBeVisible();
}

async function toTotpStep(page: Page) {
  await signUp(page);
  await passwordInput(page).fill("Tandoori-Nights-42");
  await button(page, "onboarding.password.submit").click();
  await expect(h1(page, "onboarding.totp.title")).toBeVisible();
}

async function toCodesStep(page: Page) {
  await toTotpStep(page);
  await totpInput(page).focus();
  await page.keyboard.insertText(GOOD_TOTP);
  await expect(h1(page, "onboarding.codes.title")).toBeVisible();
}

test.describe("password", () => {
  test("the field says the rule, allows paste and shows or hides the password", async ({
    page,
  }) => {
    await stubCloud(page);
    await signUp(page);
    await expect(page.getByText(t("onboarding.account_setup.progress", { n: 1 }))).toBeVisible();
    const input = passwordInput(page);
    await expect(input).toHaveAttribute("type", "password");
    await expect(input).toHaveAttribute("autocomplete", "new-password");
    await expect(input).toHaveAccessibleDescription(t("onboarding.password.hint"));
    // A paste (from a password manager) goes in as typed.
    await input.focus();
    await page.evaluate(() => {
      const el = document.activeElement as HTMLInputElement;
      const data = new DataTransfer();
      data.setData("text/plain", "pasted-Secret-99");
      const ev = new ClipboardEvent("paste", { clipboardData: data, cancelable: true });
      const allowed = el.dispatchEvent(ev);
      if (allowed) el.setRangeText("pasted-Secret-99");
      el.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await expect(input).toHaveValue("pasted-Secret-99");

    const toggle = page.getByRole("button", { name: t("ui.password.show") });
    await expect(toggle).toHaveAttribute("aria-pressed", "false");
    await toggle.click();
    const hide = page.getByRole("button", { name: t("ui.password.hide") });
    await expect(hide).toHaveAttribute("aria-pressed", "true");
    await expect(input).toHaveAttribute("type", "text");
    await hide.click();
    await expect(input).toHaveAttribute("type", "password");
    await expect(page.getByRole("button", { name: t("ui.password.show") })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  test("a short password is caught here; a common one by the cloud", async ({ page }) => {
    const cloud = await stubCloud(page);
    await signUp(page);
    await passwordInput(page).fill("abcdefghi");
    await button(page, "onboarding.password.submit").click();
    await expect(page.getByText(t("onboarding.password.too_short"), { exact: true })).toBeVisible();
    await expect(passwordInput(page)).toHaveAttribute("aria-invalid", "true");
    await expect(passwordInput(page)).toBeFocused();
    expect(cloud.passwords).toEqual([]);

    await passwordInput(page).fill("password123");
    await button(page, "onboarding.password.submit").click();
    await expect(page.getByText(t("onboarding.password.common"))).toBeVisible();
    await expect(passwordInput(page)).toBeFocused();
    expect(cloud.passwords).toEqual(["password123"]);
    await expect(h1(page, "onboarding.password.title")).toBeVisible();
  });

  test("a good password moves on to the authenticator app", async ({ page }) => {
    const cloud = await stubCloud(page);
    await toTotpStep(page);
    expect(cloud.passwords).toEqual(["Tandoori-Nights-42"]);
    expect(cloud.totpStarts).toBe(1);
    await expect(h1(page, "onboarding.totp.title")).toBeFocused();
  });
});

test.describe("authenticator app", () => {
  test("shows the QR code and the grouped key, which copies", async ({ page }) => {
    await stubCloud(page);
    await toTotpStep(page);
    await expect(page.getByText(t("onboarding.account_setup.progress", { n: 2 }))).toBeVisible();
    const qr = page.getByRole("img", { name: t("onboarding.totp.qr_alt") });
    await expect(qr).toBeVisible();
    await expect(qr).toHaveAttribute("src", /^data:image\/svg\+xml/);
    await expect(page.getByTestId("totp-secret")).toHaveText(grouped(SECRETS[0]));
    await button(page, "onboarding.totp.copy_key").click();
    await expect(page.getByText(t("onboarding.totp.copied_key"))).toBeVisible();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(SECRETS[0]);
    await expect(cells(page)).toHaveCount(6);
  });

  test("a wrong code says so inline and clears the cells; the right one shows the codes", async ({
    page,
  }) => {
    const cloud = await stubCloud(page);
    await toTotpStep(page);
    await totpInput(page).focus();
    await page.keyboard.insertText("111111");
    await expect(page.getByText(t("onboarding.totp.invalid"))).toBeVisible();
    await expect(totpInput(page)).toHaveValue("");
    await expect(totpInput(page)).toHaveAttribute("aria-invalid", "true");
    await expect(totpInput(page)).toBeFocused();
    for (let i = 0; i < 6; i++) await expect(cells(page).nth(i)).toHaveText("");
    await page.keyboard.insertText(GOOD_TOTP);
    await expect(h1(page, "onboarding.codes.title")).toBeVisible();
    expect(cloud.confirms).toEqual(["111111", GOOD_TOTP]);
  });
});

test.describe("backup codes", () => {
  test("lists the 10 codes, copies and downloads them", async ({ page }) => {
    await stubCloud(page);
    await toCodesStep(page);
    await expect(page.getByText(t("onboarding.account_setup.progress", { n: 3 }))).toBeVisible();
    const list = page.getByRole("list", { name: t("onboarding.codes.list_label") });
    await expect(list.getByRole("listitem")).toHaveText(CODES);

    await button(page, "onboarding.codes.copy").click();
    await expect(page.getByText(t("onboarding.codes.copied"))).toBeVisible();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(CODES.join("\n"));

    const [download] = await Promise.all([
      page.waitForEvent("download"),
      button(page, "onboarding.codes.download").click(),
    ]);
    expect(download.suggestedFilename()).toBe("arena-360-backup-codes.txt");
    const text = await readFile((await download.path())!, "utf8");
    for (const c of CODES) expect(text).toContain(c);
    expect(text).toContain(t("onboarding.codes.file_header"));
  });

  test("Continue needs the saved box ticked, then acks and finishes", async ({ page }) => {
    const cloud = await stubCloud(page);
    await toCodesStep(page);
    const next = button(page, "onboarding.codes.continue");
    await expect(next).toHaveAttribute("aria-disabled", "true");
    await expect(next).toHaveAccessibleDescription(t("onboarding.codes.confirm_first"));
    // aria-disabled keeps it focusable; activating it does nothing.
    await next.click({ force: true });
    await next.press("Enter");
    expect(cloud.acks).toBe(0);
    await page.getByLabel(t("onboarding.codes.saved")).check();
    await expect(next).not.toHaveAttribute("aria-disabled", "true");
    await next.click();
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
    await expect(page.getByText(t("onboarding.done.body"))).toBeVisible();
    expect(cloud.acks).toBe(1);
    expect(cloud.owner).toMatchObject({
      mfa_state: "enrolled",
      has_password: true,
      backup_codes_acked: true,
    });
  });
});

test.describe("resuming", () => {
  test("a password but no app resumes at the app with a fresh secret each visit", async ({
    page,
  }) => {
    const cloud = await stubCloud(page, { signedIn: true, owner: { has_password: true } });
    await page.goto("/account-setup");
    await expect(h1(page, "onboarding.totp.title")).toBeVisible();
    await expect(page.getByTestId("totp-secret")).toHaveText(grouped(SECRETS[0]));
    await page.reload();
    await expect(page.getByTestId("totp-secret")).toHaveText(grouped(SECRETS[1]));
    expect(cloud.totpStarts).toBe(2);
    expect(cloud.passwords).toEqual([]);
  });

  test("enrolled without saved codes explains and regenerates them", async ({ page }) => {
    const cloud = await stubCloud(page, {
      signedIn: true,
      owner: { has_password: true, mfa_state: "enrolled" },
    });
    await page.goto("/account-setup");
    await expect(h1(page, "onboarding.unacked.title")).toBeVisible();
    await expect(page.getByText(t("onboarding.unacked.intro"))).toBeVisible();
    expect(cloud.totpStarts).toBe(0);
    await button(page, "onboarding.regenerate.button").click();
    await expect(totpInput(page)).toBeFocused();
    await page.keyboard.insertText("999999");
    await expect(page.getByText(t("onboarding.regenerate.invalid"))).toBeVisible();
    await expect(totpInput(page)).toHaveValue("");
    await page.keyboard.insertText(GOOD_TOTP);
    await expect(h1(page, "onboarding.codes.title")).toBeVisible();
    await expect(
      page.getByRole("list", { name: t("onboarding.codes.list_label") }).getByRole("listitem"),
    ).toHaveText(NEW_CODES);
    await page.getByLabel(t("onboarding.codes.saved")).check();
    await button(page, "onboarding.codes.continue").click();
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
    expect(cloud.regenerates).toEqual(["999999", GOOD_TOTP]);
  });

  test("a finished setup shows done and never the codes", async ({ page }) => {
    const cloud = await stubCloud(page, {
      signedIn: true,
      owner: { has_password: true, mfa_state: "enrolled", backup_codes_acked: true },
    });
    await page.goto("/account-setup");
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
    await expect(page.getByTestId("backup-codes")).toHaveCount(0);
    expect(cloud.totpStarts + cloud.regenerates.length).toBe(0);
  });
});

test.describe("Account → Security", () => {
  test("regenerate asks for a code in a dialog, then shows the new codes once", async ({
    page,
  }) => {
    const cloud = await stubCloud(page, {
      signedIn: true,
      owner: { has_password: true, mfa_state: "enrolled", backup_codes_acked: true },
    });
    await page.goto("/account/security");
    await expect(h1(page, "onboarding.security.title")).toBeVisible();
    const open = button(page, "onboarding.regenerate.button");
    await open.click();
    const dialog = page.getByRole("dialog", { name: t("onboarding.security.dialog_title") });
    await expect(dialog).toBeVisible();
    await expect(totpInput(page)).toBeFocused();
    await page.keyboard.insertText("000000");
    await expect(dialog.getByText(t("onboarding.regenerate.invalid"))).toBeVisible();
    await expect(totpInput(page)).toHaveValue("");
    await page.keyboard.insertText(GOOD_TOTP);
    const codes = page.getByRole("dialog", { name: t("onboarding.codes.title") });
    await expect(
      codes.getByRole("list", { name: t("onboarding.codes.list_label") }).getByRole("listitem"),
    ).toHaveText(NEW_CODES);
    for (let i = 0; i < 3; i++) {
      await page.keyboard.press("Escape");
      await expect(codes).toBeVisible();
    }
    await codes.getByLabel(t("onboarding.codes.saved")).check();
    await codes.getByRole("button", { name: t("onboarding.codes.continue") }).click();
    await expect(codes).toBeHidden();
    await expect(page.getByText(t("onboarding.security.done"))).toBeVisible();
    await expect(open).toBeFocused();
    expect(cloud.regenerates).toEqual(["000000", GOOD_TOTP]);
    expect(cloud.acks).toBe(1);
  });

  test("Cancel closes the dialog without a request", async ({ page }) => {
    const cloud = await stubCloud(page, {
      signedIn: true,
      owner: { has_password: true, mfa_state: "enrolled", backup_codes_acked: true },
    });
    await page.goto("/account/security");
    await button(page, "onboarding.regenerate.button").click();
    await button(page, "onboarding.security.cancel").click();
    await expect(page.getByRole("dialog")).toBeHidden();
    expect(cloud.regenerates).toEqual([]);
  });

  test("an owner still in setup is sent to finish it", async ({ page }) => {
    await stubCloud(page, { signedIn: true });
    await page.goto("/account/security");
    await expect(page.getByText(t("onboarding.security.not_enrolled"))).toBeVisible();
    await expect(button(page, "onboarding.regenerate.button")).toHaveCount(0);
    await page.getByRole("link", { name: t("onboarding.security.finish_setup") }).click();
    await expect(h1(page, "onboarding.password.title")).toBeVisible();
  });

  test("without a session it goes back to the start", async ({ page }) => {
    await stubCloud(page);
    await page.goto("/account/security");
    await expect(page).toHaveURL(/\/$/);
  });
});

test.describe("an access cookie that expires mid-setup", () => {
  test("confirm refreshes the session once and retries", async ({ page }) => {
    const cloud = await stubCloud(page);
    await toTotpStep(page);
    cloud.nextAccount.push({ status: 401, body: err("AUTH_FAILED") });
    await totpInput(page).focus();
    await page.keyboard.insertText(GOOD_TOTP);
    await expect(h1(page, "onboarding.codes.title")).toBeVisible();
    expect(cloud.refreshes).toBe(1);
    expect(cloud.confirms).toEqual([GOOD_TOTP]);
  });

  test("ack refreshes the session once, keeping the codes on screen", async ({ page }) => {
    const cloud = await stubCloud(page);
    await toCodesStep(page);
    cloud.nextAccount.push({ status: 401, body: err("AUTH_FAILED") });
    await page.getByLabel(t("onboarding.codes.saved")).check();
    await button(page, "onboarding.codes.continue").click();
    await expect(h1(page, "onboarding.done.title")).toBeVisible();
    expect(cloud.refreshes).toBe(1);
    expect(cloud.acks).toBe(1);
  });

  test("a failed refresh goes back to the start", async ({ page }) => {
    const cloud = await stubCloud(page);
    await toCodesStep(page);
    cloud.nextAccount.push({ status: 401, body: err("AUTH_FAILED") });
    cloud.nextRefresh.push({ status: 401, body: err("AUTH_FAILED") });
    await page.getByLabel(t("onboarding.codes.saved")).check();
    await button(page, "onboarding.codes.continue").click();
    await expect(page).toHaveURL(/\/$/);
    expect(cloud.acks).toBe(0);
  });
});

test("a failed start offers Try again", async ({ page }) => {
  const cloud = await stubCloud(page, { signedIn: true, owner: { has_password: true } });
  cloud.nextAccount.push({ status: 500, body: err("INTERNAL") });
  await page.goto("/account-setup");
  await expect(page.getByText(t("onboarding.error.generic"))).toBeVisible();
  await button(page, "onboarding.account_setup.retry").click();
  await expect(page.getByTestId("totp-secret")).toHaveText(grouped(SECRETS[0]));
});

test.describe("Account → Security session", () => {
  const enrolled = {
    signedIn: true,
    owner: { has_password: true, mfa_state: "enrolled" as const, backup_codes_acked: true },
  };

  test("an expired access cookie is refreshed, not bounced", async ({ page }) => {
    const cloud = await stubCloud(page, enrolled);
    cloud.nextSession.push({ status: 401, body: err("AUTH_FAILED") });
    await page.goto("/account/security");
    await expect(button(page, "onboarding.regenerate.button")).toBeVisible();
    expect(cloud.refreshes).toBe(1);
  });

  test("offline offers Try again", async ({ page }) => {
    const cloud = await stubCloud(page, enrolled);
    cloud.nextSession.push({ status: 0, body: null, abort: true });
    await page.goto("/account/security");
    await expect(page.getByText(t("onboarding.error.offline"))).toBeVisible();
    await expect(page).toHaveURL(/\/account\/security$/);
    await button(page, "onboarding.account_setup.retry").click();
    await expect(button(page, "onboarding.regenerate.button")).toBeVisible();
  });
});
