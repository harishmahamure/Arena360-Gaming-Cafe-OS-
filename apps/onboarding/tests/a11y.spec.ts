import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { GOOD_TOTP, PASSWORD, READY_OWNER, stubCloud, t, toCodeStep, type Cloud } from "./stub";

// Accessibility floor for every onboarding view in light and dark: landmarks, one h1, labelled
// fields, targets of at least 44 × 44 px and no serious or critical axe violations.

const THEMES = ["light", "dark"] as const;
const INTERACTIVE = 'button, a, input, select, [role="tab"], [tabindex]:not([tabindex="-1"])';

const h1 = (page: Page, key: Parameters<typeof t>[0]) =>
  page.getByRole("heading", { level: 1, name: t(key) });

async function toTotp(page: Page) {
  await page.goto("/account-setup");
  await page.getByLabel(t("onboarding.password.label"), { exact: true }).fill("Tandoori-42-x");
  await page.getByRole("button", { name: t("onboarding.password.submit") }).click();
  await expect(h1(page, "onboarding.totp.title")).toBeVisible();
}

async function toCodes(page: Page) {
  await toTotp(page);
  await page.getByLabel(t("onboarding.totp.code_label"), { exact: true }).focus();
  await page.keyboard.insertText(GOOD_TOTP);
  await expect(h1(page, "onboarding.codes.title")).toBeVisible();
}

const ENROLLED = { has_password: true, mfa_state: "enrolled" as const };

async function toSigninChallenge(page: Page) {
  await page.goto("/sign-in");
  await page.getByLabel(t("onboarding.signin.login"), { exact: true }).fill("+919876543210");
  await page.getByLabel(t("onboarding.password.label"), { exact: true }).fill(PASSWORD);
  await page.getByRole("button", { name: t("onboarding.signin.submit"), exact: true }).click();
  await expect(h1(page, "onboarding.signin.challenge_title")).toBeVisible();
}

const VIEWS: {
  name: string;
  cloud?: Parameters<typeof stubCloud>[1];
  open: (page: Page, cloud: Cloud) => Promise<void>;
}[] = [
  {
    name: "get started",
    open: async (page) => {
      await page.goto("/");
      await page.getByRole("button", { name: t("onboarding.start.send") }).click();
    },
  },
  { name: "code step", open: (page) => toCodeStep(page) },
  {
    name: "account setup",
    open: async (page) => {
      await toCodeStep(page);
      await page.keyboard.insertText("123456");
      await expect(
        page.getByRole("heading", { level: 1, name: t("onboarding.password.title") }),
      ).toBeVisible();
      // With the password shown and an error.
      await page.getByRole("button", { name: t("ui.password.show") }).click();
      await page.getByRole("button", { name: t("onboarding.password.submit") }).click();
      await expect(
        page.getByText(t("onboarding.password.too_short"), { exact: true }),
      ).toBeVisible();
    },
  },
  { name: "authenticator app", cloud: { signedIn: true }, open: toTotp },
  {
    name: "authenticator app, wrong code",
    cloud: { signedIn: true },
    open: async (page) => {
      await toTotp(page);
      await page.getByLabel(t("onboarding.totp.code_label"), { exact: true }).focus();
      await page.keyboard.insertText("111111");
      await expect(page.getByText(t("onboarding.totp.invalid"))).toBeVisible();
    },
  },
  { name: "backup codes", cloud: { signedIn: true }, open: toCodes },
  {
    name: "codes not saved",
    cloud: { signedIn: true, owner: ENROLLED },
    open: async (page) => {
      await page.goto("/account-setup");
      await page.getByRole("button", { name: t("onboarding.regenerate.button") }).click();
      await expect(page.getByLabel(t("onboarding.totp.code_label"), { exact: true })).toBeFocused();
    },
  },
  {
    name: "done",
    cloud: { signedIn: true, owner: { ...ENROLLED, backup_codes_acked: true } },
    open: async (page) => {
      await page.goto("/account-setup");
      await expect(h1(page, "onboarding.done.title")).toBeVisible();
    },
  },
  {
    name: "security",
    cloud: { signedIn: true, owner: { ...ENROLLED, backup_codes_acked: true } },
    open: async (page) => {
      await page.goto("/account/security");
      await expect(
        page.getByRole("button", { name: t("onboarding.regenerate.button") }),
      ).toBeVisible();
      await expect(page.getByRole("listitem")).toHaveCount(2);
    },
  },
  {
    name: "sign in",
    open: async (page) => {
      await page.goto("/sign-in");
      await page.getByRole("button", { name: t("onboarding.signin.submit"), exact: true }).click();
      await expect(page.getByText(t("onboarding.signin.login_required"))).toBeVisible();
    },
  },
  {
    name: "sign in, failed",
    open: async (page) => {
      await page.goto("/sign-in");
      await page.getByLabel(t("onboarding.signin.login"), { exact: true }).fill("a@b.in");
      await page.getByLabel(t("onboarding.password.label"), { exact: true }).fill("wrong-one-1");
      await page.getByRole("button", { name: t("onboarding.signin.submit"), exact: true }).click();
      await expect(page.getByText(t("onboarding.signin.failed"))).toBeVisible();
    },
  },
  {
    name: "sign-in code",
    cloud: { owner: { ...ENROLLED, backup_codes_acked: true, sms_fallback: true } },
    open: toSigninChallenge,
  },
  {
    name: "sign-in backup code",
    cloud: { owner: { ...ENROLLED, backup_codes_acked: true } },
    open: async (page) => {
      await toSigninChallenge(page);
      await page.getByRole("button", { name: t("onboarding.signin.use_backup") }).click();
      await page.getByRole("button", { name: t("onboarding.signin.submit"), exact: true }).click();
      await expect(page.getByText(t("onboarding.signin.backup_required"))).toBeVisible();
    },
  },
  {
    name: "sign-in text message",
    cloud: { owner: { ...ENROLLED, backup_codes_acked: true, sms_fallback: true } },
    open: async (page) => {
      await toSigninChallenge(page);
      await page.getByRole("button", { name: t("onboarding.signin.text_me") }).click();
      await expect(
        page.getByLabel(t("onboarding.signin.sms_label"), { exact: true }),
      ).toBeFocused();
    },
  },
  {
    name: "backup codes low",
    cloud: { owner: { ...ENROLLED, backup_codes_acked: true } },
    open: async (page, cloud) => {
      cloud.backupRemaining = 3;
      await toSigninChallenge(page);
      await page.getByRole("button", { name: t("onboarding.signin.use_backup") }).click();
      await page
        .getByLabel(t("onboarding.signin.backup_label"), { exact: true })
        .fill("ABCDE-FG000");
      await page.getByRole("button", { name: t("onboarding.signin.submit"), exact: true }).click();
      await expect(
        page.getByRole("heading", { level: 1, name: t("onboarding.signin.low_title", { n: 3 }) }),
      ).toBeVisible();
    },
  },
  {
    name: "security, sign out everywhere",
    cloud: { signedIn: true, owner: { ...ENROLLED, backup_codes_acked: true } },
    open: async (page) => {
      await page.goto("/account/security");
      await page.getByRole("button", { name: t("onboarding.security.signout_button") }).click();
      await expect(
        page.getByRole("dialog", { name: t("onboarding.security.signout_confirm_title") }),
      ).toBeVisible();
    },
  },
  {
    name: "security, new codes",
    cloud: { signedIn: true, owner: { ...ENROLLED, backup_codes_acked: true } },
    open: async (page) => {
      await page.goto("/account/security");
      await page.getByRole("button", { name: t("onboarding.regenerate.button") }).click();
      await page.keyboard.insertText(GOOD_TOTP);
      await expect(page.getByRole("dialog", { name: t("onboarding.codes.title") })).toBeVisible();
    },
  },
  {
    name: "setup, café step with GST",
    cloud: { signedIn: true, owner: READY_OWNER },
    open: async (page) => {
      await page.goto("/setup");
      await expect(h1(page, "ui.wizard.step.cafe")).toBeVisible();
      await page.getByLabel(t("onboarding.cafe.gst.yes"), { exact: true }).check();
      await page.getByRole("button", { name: t("onboarding.cafe.save"), exact: true }).click();
      await expect(page.getByText(t("onboarding.cafe.error.gstin_required"))).toBeVisible();
    },
  },
  {
    name: "setup, editing the branch code",
    cloud: { signedIn: true, owner: READY_OWNER, wizard: { branch: {} } },
    open: async (page) => {
      await page.goto("/setup/cafe");
      await page.getByRole("button", { name: t("onboarding.cafe.code.edit"), exact: true }).click();
      await page.getByLabel(t("onboarding.cafe.code.field"), { exact: true }).fill("ab");
      await page.getByRole("button", { name: t("onboarding.cafe.code.save"), exact: true }).click();
      await expect(page.getByText(t("onboarding.cafe.code.invalid"))).toBeVisible();
    },
  },
  {
    name: "setup, a coming-soon step",
    cloud: {
      signedIn: true,
      owner: READY_OWNER,
      wizard: { branch: {}, done: ["cafe", "tier", "install_hub", "devices"] },
    },
    open: async (page) => {
      await page.goto("/setup");
      await expect(h1(page, "ui.wizard.step.rates")).toBeVisible();
    },
  },
  {
    name: "setup, install the Hub with a live code",
    cloud: { signedIn: true, owner: READY_OWNER, wizard: { branch: {}, done: ["cafe", "tier"] } },
    open: async (page) => {
      await page.goto("/setup/install_hub");
      await expect(page.getByTestId("pairing-code-value")).toBeVisible();
      await page.getByRole("button", { name: t("ui.pairing_code.copy"), exact: true }).click();
      await expect(page.getByTestId("pairing-code-status")).not.toBeEmpty();
    },
  },
  {
    name: "setup, install the Hub with an expired code",
    cloud: { signedIn: true, owner: READY_OWNER, wizard: { branch: {}, done: ["cafe", "tier"] } },
    open: async (page, cloud) => {
      const start = Date.UTC(2026, 9, 25, 9, 0);
      cloud.clockMs = start;
      await page.clock.install({ time: start });
      await page.goto("/setup/install_hub");
      await expect(page.getByTestId("pairing-code-value")).toBeVisible();
      await page.clock.runFor(15 * 60 * 1000);
      await expect(page.getByTestId("pairing-code-expired")).toBeVisible();
    },
  },
  {
    name: "setup, plan step, yearly",
    cloud: { signedIn: true, owner: READY_OWNER, wizard: { branch: {} } },
    open: async (page) => {
      await page.goto("/setup/tier");
      await expect(h1(page, "ui.wizard.step.tier")).toBeVisible();
      await page.getByRole("tab", { name: t("onboarding.tier.yearly") }).click();
      await expect(page.getByTestId("plan-free")).toBeVisible();
    },
  },
  {
    name: "setup, plan step, no Free left",
    cloud: {
      signedIn: true,
      owner: READY_OWNER,
      wizard: { branch: {}, otherFree: true },
    },
    open: async (page) => {
      await page.goto("/setup/tier");
      await expect(page.getByText(t("onboarding.tier.choose_first"))).toBeVisible();
    },
  },
];

for (const theme of THEMES) {
  for (const view of VIEWS) {
    test(`${view.name} (${theme}): landmarks, one h1, labels, targets and axe`, async ({
      page,
    }) => {
      const cloud = await stubCloud(page, view.cloud);
      await page.addInitScript((v) => localStorage.setItem("arena.theme", v), theme);
      await view.open(page, cloud);
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      await page.evaluate(() => document.fonts.ready);

      await expect(page.locator("header")).toHaveCount(1);
      await expect(page.locator("main")).toHaveCount(1);
      await expect(page.locator("footer")).toHaveCount(1);
      await expect(page.locator("h1")).toHaveCount(1);

      const unlabelled = await page.evaluate(() =>
        [...document.querySelectorAll("input, select, textarea")]
          .filter((el) => {
            const id = el.id;
            const label = id ? document.querySelector(`label[for="${id}"]`) : null;
            return !label?.textContent?.trim() && !el.getAttribute("aria-label");
          })
          .map((el) => el.outerHTML.slice(0, 80)),
      );
      expect(unlabelled, unlabelled.join("\n")).toEqual([]);

      const controls = page.locator(INTERACTIVE);
      const small: string[] = [];
      for (let i = 0; i < (await controls.count()); i++) {
        const box = await controls.nth(i).boundingBox();
        if (!box) continue;
        if (box.width < 44 || box.height < 44) {
          const what = await controls.nth(i).evaluate((el) => el.outerHTML.slice(0, 80));
          small.push(`${what} is ${box.width.toFixed(1)}×${box.height.toFixed(1)}`);
        }
      }
      expect(small, small.join("\n")).toEqual([]);

      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
      );
      expect(overflow, "no horizontal scroll").toBe(false);

      const results = await new AxeBuilder({ page }).analyze();
      const blocking = results.violations.filter(
        (v) => v.impact === "serious" || v.impact === "critical",
      );
      const report = blocking.map(
        (v) => `${v.id} (${v.impact}): ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
      );
      expect(report, report.join("\n")).toEqual([]);
    });
  }
}
