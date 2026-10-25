import { expect, test, type Page } from "@playwright/test";
import { PAIR_CODES, READY_OWNER, err, stubCloud, t } from "./stub";

type Key = Parameters<typeof t>[0];
const h1 = (page: Page, key: Key) => page.getByRole("heading", { level: 1, name: t(key) });
const ready = {
  signedIn: true,
  owner: READY_OWNER,
  wizard: { branch: {}, done: ["cafe", "tier"] as ("cafe" | "tier")[] },
};
const START = Date.UTC(2026, 9, 25, 9, 0);
const MIN = 60 * 1000;
const value = (page: Page) => page.getByTestId("pairing-code-value");
const button = (page: Page, key: Key) => page.getByRole("button", { name: t(key), exact: true });
const grouped = (code: string) => `${code.slice(0, 4)} ${code.slice(4)}`;

async function openWithClock(page: Page, clientAheadMs = 0) {
  const cloud = await stubCloud(page, ready);
  cloud.clockMs = START;
  await page.clock.install({ time: START + clientAheadMs });
  await page.clock.pauseAt(START + clientAheadMs + 1);
  await page.goto("/setup/install_hub");
  await expect(value(page)).toBeVisible();
  return cloud;
}

async function recordAnnouncements(page: Page) {
  await page.evaluate(() => {
    const el = document.querySelector('[data-testid="pairing-code-announcement"]')!;
    const w = window as unknown as { said: string[] };
    w.said = [];
    new MutationObserver(() => {
      const text = el.textContent?.trim() ?? "";
      if (text && w.said.at(-1) !== text) w.said.push(text);
    }).observe(el, { childList: true, characterData: true, subtree: true });
  });
}

const said = (page: Page) =>
  page.evaluate(() => (window as unknown as { said: string[] }).said.slice());

test("step 3 shows the three parts and a code grouped 4 + 4 with a 15:00 countdown", async ({
  page,
}) => {
  const cloud = await openWithClock(page);
  await expect(h1(page, "ui.wizard.step.install_hub")).toBeVisible();
  for (const key of [
    "onboarding.install.download.title",
    "onboarding.install.install.title",
    "onboarding.install.code.title",
  ] as const) {
    await expect(page.getByRole("heading", { level: 2, name: t(key) })).toBeVisible();
  }
  await expect(page.getByText(t("onboarding.install.download.platform"))).toBeVisible();
  await expect(
    page.getByRole("link", { name: t("onboarding.install.download.link") }),
  ).toHaveAttribute("href", "/download/hub");
  await expect(
    page.getByRole("link", { name: t("onboarding.install.download.android") }),
  ).toHaveAttribute("href", "/help/android-hub");

  await expect(value(page)).toHaveText(grouped(PAIR_CODES[0]));
  const groups = await value(page).locator("span").allTextContents();
  expect(groups).toEqual([PAIR_CODES[0].slice(0, 4), PAIR_CODES[0].slice(4)]);
  expect(await value(page).evaluate((el) => getComputedStyle(el).fontVariantNumeric)).toContain(
    "tabular-nums",
  );
  await expect(page.getByText(t("ui.pairing_code.single_use"), { exact: true })).toBeVisible();
  await expect(page.getByTestId("pairing-code-expires")).toHaveText(
    t("ui.pairing_code.expires_in", { time: "15:00" }),
  );
  await page.clock.runFor(1000);
  await expect(page.getByTestId("pairing-code-expires")).toHaveText(
    t("ui.pairing_code.expires_in", { time: "14:59" }),
  );
  await expect(button(page, "ui.pairing_code.copy")).toBeVisible();
  expect(cloud.pairGets).toBe(1);
  expect(cloud.pairCodes.length).toBe(1);
  expect(cloud.done.has("install_hub")).toBe(false);

  const main = page.locator("main");
  await expect(main.locator("canvas")).toHaveCount(0);
  await expect(main.locator("img")).toHaveCount(0);
  await expect(main.locator("svg image, object, iframe")).toHaveCount(0);
  const imgs = await page.evaluate(() =>
    [...document.querySelectorAll("img, canvas")].map((el) => el.outerHTML),
  );
  for (const html of imgs) expect(html).not.toContain(PAIR_CODES[0]);
});

test("reloading the page issues a new code", async ({ page }) => {
  const cloud = await stubCloud(page, ready);
  await page.goto("/setup/install_hub");
  await expect(value(page)).toHaveText(grouped(PAIR_CODES[0]));
  await page.reload();
  await expect(value(page)).toHaveText(grouped(PAIR_CODES[1]));
  expect(cloud.pairGets).toBe(2);
  expect(cloud.pairCodes.map((c) => c.code)).toEqual([PAIR_CODES[0], PAIR_CODES[1]]);
});

test.describe("copy", () => {
  test.use({ permissions: ["clipboard-read", "clipboard-write"] });

  test("at expiry a stale copied status clears and focus moves to the new primary action", async ({
    page,
  }) => {
    await openWithClock(page);
    await button(page, "ui.pairing_code.copy").click();
    await expect(page.getByTestId("pairing-code-status")).toHaveText(t("ui.pairing_code.copied"));
    await expect(button(page, "ui.pairing_code.copy")).toBeFocused();
    await page.clock.runFor(15 * MIN);
    await expect(page.getByTestId("pairing-code-expired")).toBeVisible();
    await expect(page.getByTestId("pairing-code-status")).toHaveText("");
    await expect(button(page, "ui.pairing_code.regenerate")).toBeFocused();
  });

  test("copying writes the code and says so politely", async ({ page }) => {
    await stubCloud(page, ready);
    await page.goto("/setup/install_hub");
    await button(page, "ui.pairing_code.copy").click();
    const status = page.getByTestId("pairing-code-status");
    await expect(status).toHaveText(t("ui.pairing_code.copied"));
    await expect(status).toHaveAttribute("role", "status");
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(PAIR_CODES[0]);
  });
});

test("when the clipboard refuses, the code is selected with a Ctrl+C hint", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: () => Promise.reject(new Error("denied")) },
    });
  });
  await stubCloud(page, ready);
  await page.goto("/setup/install_hub");
  await button(page, "ui.pairing_code.copy").click();
  await expect(page.getByTestId("pairing-code-status")).toHaveText(t("ui.pairing_code.copy_hint"));
  await expect(value(page)).toBeFocused();
  const selected = await page.evaluate(() => window.getSelection()?.toString() ?? "");
  expect(selected.replace(/\s/g, "")).toBe(PAIR_CODES[0]);
});

test("the countdown is announced only at 10, 5 and 1 minutes and at expiry", async ({ page }) => {
  await openWithClock(page);
  const live = page.getByTestId("pairing-code-announcement");
  await expect(live).toHaveAttribute("aria-live", "polite");
  await expect(page.getByTestId("pairing-code-expires")).not.toHaveAttribute("aria-live");
  await recordAnnouncements(page);

  await page.clock.runFor(5 * MIN - 1000);
  await expect(page.getByTestId("pairing-code-expires")).toHaveText(
    t("ui.pairing_code.expires_in", { time: "10:01" }),
  );
  expect(await said(page)).toEqual([]);
  await page.clock.runFor(1000);
  await expect(live).toHaveText(t("ui.pairing_code.minutes_left", { n: 10 }));

  await page.clock.runFor(5 * MIN - 1000);
  expect(await said(page)).toEqual([t("ui.pairing_code.minutes_left", { n: 10 })]);
  await page.clock.runFor(1000);
  await expect(live).toHaveText(t("ui.pairing_code.minutes_left", { n: 5 }));

  await page.clock.runFor(4 * MIN);
  await expect(live).toHaveText(t("ui.pairing_code.minute_left"));
  await page.clock.runFor(MIN - 1000);
  await expect(page.getByTestId("pairing-code-expires")).toHaveText(
    t("ui.pairing_code.expires_in", { time: "00:01" }),
  );
  await page.clock.runFor(1000);
  await expect(live).toHaveText(t("ui.pairing_code.expired_message"));
  await page.clock.runFor(2 * MIN);

  expect(await said(page)).toEqual([
    t("ui.pairing_code.minutes_left", { n: 10 }),
    t("ui.pairing_code.minutes_left", { n: 5 }),
    t("ui.pairing_code.minute_left"),
    t("ui.pairing_code.expired_message"),
  ]);
});

test("at zero the code is struck through and a new code is the one primary action", async ({
  page,
}) => {
  const cloud = await openWithClock(page);
  await page.clock.runFor(15 * MIN);
  await expect(value(page)).toHaveAttribute("data-expired", "true");
  expect(await value(page).evaluate((el) => getComputedStyle(el).textDecorationLine)).toBe(
    "line-through",
  );
  const badge = page.locator('[data-component="status-badge"]');
  await expect(badge).toHaveText(t("ui.pairing_code.expired"));
  await expect(page.getByTestId("pairing-code-expired")).toHaveText(
    t("ui.pairing_code.expired_message"),
  );
  await expect(page.getByTestId("pairing-code-expires")).toHaveCount(0);
  await expect(button(page, "ui.pairing_code.copy")).toHaveCount(0);
  const primaries = page.locator("main .button--primary");
  await expect(primaries).toHaveCount(1);
  await expect(primaries).toHaveText(t("ui.pairing_code.regenerate"));

  cloud.clockMs = START + 15 * MIN;
  await button(page, "ui.pairing_code.regenerate").click();
  await expect(value(page)).toHaveText(grouped(PAIR_CODES[1]));
  await expect(value(page)).not.toHaveAttribute("data-expired");
  await expect(value(page)).toBeFocused();
  await expect(page.getByTestId("pairing-code-expires")).toHaveText(
    t("ui.pairing_code.expires_in", { time: "15:00" }),
  );
  await expect(page.getByTestId("install-status")).toHaveText(t("onboarding.install.new_code"));
  await expect(badge).toHaveCount(0);
  await expect(page.getByTestId("pairing-code-announcement")).toHaveText("");
  expect(cloud.pairCodes.length).toBe(2);
});

test("a live code can be replaced before it expires", async ({ page }) => {
  const cloud = await openWithClock(page);
  await page.clock.runFor(6 * MIN);
  await expect(page.getByTestId("pairing-code-announcement")).not.toHaveText("");
  cloud.clockMs = START + 6 * MIN;
  await button(page, "ui.pairing_code.regenerate").click();
  await expect(value(page)).toHaveText(grouped(PAIR_CODES[1]));
  await expect(page.getByTestId("pairing-code-expires")).toHaveText(
    t("ui.pairing_code.expires_in", { time: "15:00" }),
  );
  await expect(page.getByTestId("pairing-code-announcement")).toHaveText("");
});

test("without a plan the step says what to finish first", async ({ page }) => {
  const cloud = await stubCloud(page, {
    ...ready,
    wizard: { branch: {}, done: ["cafe", "install_hub"] },
  });
  await page.goto("/setup/install_hub");
  await expect(page.getByText(t("onboarding.install.error.not_ready"))).toBeVisible();
  await expect(value(page)).toHaveCount(0);
  expect(cloud.pairCodes).toEqual([]);
});

test("focus on Generate moves to the new primary Generate at expiry", async ({ page }) => {
  await openWithClock(page);
  await button(page, "ui.pairing_code.regenerate").focus();
  await page.clock.runFor(15 * MIN);
  await expect(page.locator("main .button--primary")).toBeFocused();
});

test("the countdown follows the server clock, not a skewed client clock", async ({ page }) => {
  await openWithClock(page, 5 * MIN);
  await expect(page.getByTestId("pairing-code-expires")).toHaveText(
    t("ui.pairing_code.expires_in", { time: "15:00" }),
  );
  await page.clock.runFor(MIN);
  await expect(page.getByTestId("pairing-code-expires")).toHaveText(
    t("ui.pairing_code.expires_in", { time: "14:00" }),
  );
});

test("a backgrounded tab corrects the countdown on visibility change and focus", async ({
  page,
}) => {
  await openWithClock(page);
  await page.clock.setSystemTime(START + 6 * MIN + 1);
  await expect(page.getByTestId("pairing-code-expires")).toHaveText(
    t("ui.pairing_code.expires_in", { time: "15:00" }),
  );
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  await expect(page.getByTestId("pairing-code-expires")).toHaveText(
    t("ui.pairing_code.expires_in", { time: "09:00" }),
  );
  await page.clock.setSystemTime(START + 8 * MIN + 1);
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(page.getByTestId("pairing-code-expires")).toHaveText(
    t("ui.pairing_code.expires_in", { time: "07:00" }),
  );
});

test("an already paired Hub shows the paired message and issues no code", async ({ page }) => {
  const cloud = await stubCloud(page, { ...ready, wizard: { ...ready.wizard, hubPaired: true } });
  await page.goto("/setup/install_hub");
  await expect(page.getByText(t("onboarding.install.paired"))).toBeVisible();
  expect(cloud.pairGets).toBe(1);
  expect(cloud.pairCodes).toEqual([]);
  await expect(value(page)).toHaveCount(0);
});

test("too many new codes keeps the live code and says when to try again", async ({ page }) => {
  const cloud = await stubCloud(page, ready);
  await page.goto("/setup/install_hub");
  await expect(value(page)).toHaveText(grouped(PAIR_CODES[0]));
  cloud.nextWizard.push({
    status: 429,
    body: err("RATE_LIMITED"),
    headers: { "retry-after": "125" },
  });
  await button(page, "ui.pairing_code.regenerate").click();
  await expect(page.getByRole("alert")).toContainText(
    t("onboarding.install.error.rate_limited", { time: "2:05" }),
  );
  await expect(value(page)).toHaveText(grouped(PAIR_CODES[0]));
  expect(cloud.pairCodes.length).toBe(1);
});

test("a lost regenerate response drops the shown code and reloads the step", async ({ page }) => {
  const cloud = await stubCloud(page, ready);
  await page.goto("/setup/install_hub");
  await expect(value(page)).toHaveText(grouped(PAIR_CODES[0]));
  cloud.nextWizard.push({ status: 0, body: null, abort: true });
  await button(page, "ui.pairing_code.regenerate").click();
  await expect(value(page)).toHaveText(grouped(PAIR_CODES[1]));
  expect(cloud.pairGets).toBe(2);
  expect(cloud.pairCodes.map((c) => c.code)).toEqual([PAIR_CODES[0], PAIR_CODES[1]]);
});

const nextHeading = (page: Page) => h1(page, "ui.wizard.step.devices");

async function fakeEventSource(page: Page) {
  await page.addInitScript(() => {
    const w = window as unknown as { sources: FakeSource[] };
    class FakeSource {
      listeners: Record<string, () => void> = {};
      onopen: (() => void) | null = null;
      onerror: (() => void) | null = null;
      constructor() {
        w.sources.push(this);
        setTimeout(() => this.onopen?.());
      }
      addEventListener(type: string, fn: () => void) {
        this.listeners[type] = fn;
      }
      close() {}
    }
    w.sources = [];
    (window as unknown as { EventSource: unknown }).EventSource = FakeSource;
  });
}

test("the first heartbeat moves step 3 to step 4 through the event stream", async ({ page }) => {
  await fakeEventSource(page);
  const cloud = await stubCloud(page, ready);
  await page.goto("/setup/install_hub");
  await expect(value(page)).toBeVisible();
  cloud.done.add("install_hub");
  await page.evaluate(() =>
    (
      window as unknown as { sources: { listeners: Record<string, () => void> }[] }
    ).sources[0].listeners["wizard.changed"](),
  );
  await expect(nextHeading(page)).toBeFocused();
  await expect(page).toHaveURL(/\/setup\/devices$/);
  await expect(page.getByTestId("wizard-announcement")).toHaveText(
    t("onboarding.setup.announce", { n: 4, total: 10, name: t("ui.wizard.step.devices") }),
  );
});

test("without the event stream, polling moves step 3 to step 4 within 5 s", async ({ page }) => {
  await page.clock.install({ time: START });
  const cloud = await stubCloud(page, ready);
  await page.goto("/setup/install_hub");
  await expect(value(page)).toBeVisible();
  cloud.done.add("install_hub");
  await page.clock.runFor(5000);
  await expect(nextHeading(page)).toBeFocused();
  await expect(page).toHaveURL(/\/setup\/devices$/);
});
