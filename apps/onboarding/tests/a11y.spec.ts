import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { GOOD_TOTP, stubCloud, t, toCodeStep, type Cloud } from "./stub";

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
