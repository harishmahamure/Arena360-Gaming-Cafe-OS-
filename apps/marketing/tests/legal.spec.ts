import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { DESKTOP, open, t } from "./helpers";

// FAQ and Legal: grouped disclosure list with the Pricing FAQ's refund text and `FAQPage` data;
// the Data ownership and Ad policy statements; four Legal pages with a version, an effective date
// and a change history; and the Privacy policy's version equal to the sign-up policy-version
// constant in `arena-proto`.

const GROUPS = ["billing", "data", "offline", "ads", "refunds"];
const PAGES = [
  { slug: "terms", title: "Terms of service" },
  { slug: "privacy", title: "Privacy policy" },
  { slug: "data-processing", title: "Data processing terms" },
  { slug: "ad-policy", title: "Ad policy" },
];
const SOURCE = new URL("../../../crates/arena-proto/src/player_signup.rs", import.meta.url);

test("the FAQ groups answers under Billing, Data ownership, Offline, Ads and Refunds", async ({
  page,
}) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/faq");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("site.faq.heading"));
  await expect(page.getByRole("heading", { level: 2 })).toHaveText([
    t("site.faq.group.billing"),
    t("site.faq.group.data"),
    t("site.faq.group.offline"),
    t("site.faq.group.ads"),
    t("site.faq.group.refunds"),
  ]);
  for (const id of GROUPS)
    await expect(page.locator(`section#${id} details`).first()).toBeVisible();
  // A disclosure list: closed until opened, and keyboard operable.
  const answer = page.getByText(t("site.faq.ads.remove.a"), { exact: true });
  await expect(answer).toBeHidden();
  await page.getByText(t("site.faq.ads.remove.q")).focus();
  await page.keyboard.press("Enter");
  await expect(answer).toBeVisible();
});

test("the refund answer is the Pricing FAQ's text", async ({ page }) => {
  await open(page, "/faq");
  const refund = t("site.pricing.faq.refund.a");
  await page.getByText(t("site.pricing.faq.refund.q")).click();
  await expect(page.locator("section#refunds").getByText(refund, { exact: true })).toBeVisible();
  await open(page, "/pricing");
  await page.getByText(t("site.pricing.faq.refund.q")).click();
  await expect(page.getByText(refund, { exact: true })).toBeVisible();
});

test("the FAQ emits FAQPage structured data for every question", async ({ page }) => {
  await open(page, "/faq");
  const raw = await page.locator('script[type="application/ld+json"]').textContent();
  const data = JSON.parse(raw ?? "{}");
  expect(data["@type"]).toBe("FAQPage");
  const count = await page.locator(".faq details").count();
  expect(data.mainEntity).toHaveLength(count);
  expect(data.mainEntity[0]["@type"]).toBe("Question");
  const refund = data.mainEntity.find(
    (q: { name: string }) => q.name === t("site.pricing.faq.refund.q"),
  );
  expect(refund.acceptedAnswer.text).toBe(t("site.pricing.faq.refund.a"));
});

test("the Data ownership answers say what the contract promises", async ({ page }) => {
  await open(page, "/faq");
  const section = page.locator("section#data");
  // Text inside a closed disclosure is not "visible", so read it from the DOM.
  const text = ((await section.textContent()) ?? "").toLowerCase();
  expect(text).toContain(
    "export the full branch database and every report at any time, on every plan",
  );
  expect(text).toContain("your own hub");
  expect(text).toContain("hosted in india");
});

test("Legal lists the four policies, each with a version and an effective date", async ({
  page,
}) => {
  await open(page, "/legal");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("site.legal.heading"));
  const items = page.getByRole("list", { name: t("site.legal.list") }).locator("li");
  await expect(items).toHaveCount(4);
  for (const { slug, title } of PAGES) {
    const item = page.locator(`li[data-slug="${slug}"]`);
    await expect(item.getByRole("link", { name: title })).toHaveAttribute("href", `/legal/${slug}`);
    await expect(item).toContainText(t("site.legal.version", { version: "1" }));
    await expect(item).toContainText(t("site.legal.effective", { date: "31 Oct 2026" }));
  }
});

test("each policy is its own page with a version, an effective date and a change history", async ({
  page,
}) => {
  for (const { slug, title } of PAGES) {
    const response = await page.goto(`/legal/${slug}`);
    expect(response?.status(), slug).toBe(200);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(title);
    await expect(page.locator('[data-field="version"]')).toHaveText(
      t("site.legal.version", { version: "1" }),
    );
    await expect(page.locator('[data-field="effective"]')).toHaveText(
      t("site.legal.effective", { date: "31 Oct 2026" }),
    );
    await expect(
      page.getByRole("heading", { level: 2, name: t("site.legal.history") }),
    ).toBeVisible();
    await expect(page.locator(".legal-history li").first()).toContainText("First published.");
  }
});

test("the Privacy policy's version is the sign-up policy-version constant", async ({ page }) => {
  const constant = /PLAYER_POLICY_VERSION\s*:\s*&str\s*=\s*"([^"]+)"/.exec(
    readFileSync(SOURCE, "utf8"),
  )?.[1];
  expect(constant).toBeTruthy();
  await page.goto("/legal/privacy");
  await expect(page.locator('[data-field="version"]')).toHaveText(
    t("site.legal.version", { version: constant ?? "" }),
  );
});

test("the Ad policy states where ads appear and what is excluded", async ({ page }) => {
  await page.goto("/legal/ad-policy");
  const text = (await page.locator(".legal-body").innerText()).toLowerCase();
  expect(text).toContain("only on the free android hub, at the counter");
  expect(text).toContain("never appear on player screens");
  expect(text).toContain("adult content, gambling and alcohol are excluded");
  expect(text).toContain("removed within one heartbeat of upgrading");
});

test("the sitemap lists the FAQ, Legal and every policy", async ({ request }) => {
  const body = await (await request.get("/sitemap.xml")).text();
  for (const path of ["/faq", "/legal", ...PAGES.map((p) => `/legal/${p.slug}`)])
    expect(body).toContain(`<loc>https://arena360.cloud${path}</loc>`);
});
