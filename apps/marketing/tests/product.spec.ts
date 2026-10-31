import { expect, test } from "@playwright/test";
import { DESKTOP, PHONE, open, t } from "./helpers";

// Product: the overview and in-page links, four sections with a walkthrough of 3-5 steps and a
// demo video each, the video rules (poster first, captions, native controls, a plain fallback) and
// the Console section's honest note.

const PRODUCTS = [
  { id: "hub", name: "Hub", steps: 5 },
  { id: "kiosk", name: "Kiosk", steps: 4 },
  { id: "console", name: "Console", steps: 3 },
  { id: "owner", name: "Owner", steps: 4 },
] as const;
const MEDIA = "https://media.arena360.cloud/marketing/product";

test("the overview links to a section each for Hub, Kiosk, Console and Owner", async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/product");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(t("site.product.heading"));
  const jump = page.getByRole("navigation", { name: t("site.product.jump") });
  await expect(jump.getByRole("link")).toHaveCount(4);
  for (const { id, name } of PRODUCTS) {
    await expect(jump.locator(`a[href="#${id}"]`)).toHaveText(new RegExp(`^${name}:`));
    await expect(page.locator(`section#${id}`).getByRole("heading", { level: 2 })).toHaveText(name);
  }
  await jump.locator('a[href="#console"]').click();
  await expect(page).toHaveURL(/\/product#console$/);
  await expect(page.locator("section#console")).toBeInViewport();
});

test("each section has an illustrated walkthrough of three to five steps", async ({ page }) => {
  await page.setViewportSize(DESKTOP);
  await open(page, "/product");
  for (const { id, steps } of PRODUCTS) {
    const items = page.locator(`section#${id} .product-step`);
    await expect(items).toHaveCount(steps);
    expect(steps).toBeGreaterThanOrEqual(3);
    expect(steps).toBeLessThanOrEqual(5);
    await expect(items.first().locator("svg[aria-hidden='true']")).toBeVisible();
    await expect(items.first().locator(".step-number")).toHaveText(
      t("site.product.step", { n: 1 }),
    );
  }
});

test("a demo shows its poster and loads and plays nothing until pressed", async ({ page }) => {
  const requested: string[] = [];
  page.on("request", (request) => {
    if (request.url().startsWith(MEDIA)) requested.push(request.url());
  });
  await open(page, "/product");
  for (const { id, name } of PRODUCTS) {
    const video = page.locator(`section#${id} video`);
    await expect(video).toHaveAttribute("poster", `${MEDIA}/${id}/poster.jpg`);
    await expect(video).toHaveAttribute("preload", "none");
    await expect(video).toHaveAttribute("controls", "");
    await expect(video).not.toHaveAttribute("autoplay", /.*/);
    await expect(video).not.toHaveAttribute("muted", /.*/);
    await expect(video).toHaveAccessibleName(t("site.product.video.player", { product: name }));
    expect(await video.evaluate((el: HTMLVideoElement) => el.paused)).toBe(true);
    expect(await video.evaluate((el: HTMLVideoElement) => el.muted)).toBe(false);
  }
  expect(requested.filter((url) => /\.(webm|mp4)$/.test(url))).toEqual([]);
});

test("a demo has English captions on by default and a text transcript link", async ({ page }) => {
  await open(page, "/product");
  for (const { id, name } of PRODUCTS) {
    const section = page.locator(`section#${id}`);
    const track = section.locator("video track");
    await expect(track).toHaveAttribute("kind", "captions");
    await expect(track).toHaveAttribute("srclang", "en");
    await expect(track).toHaveAttribute("src", `${MEDIA}/${id}/captions.en.vtt`);
    await expect(track).toHaveAttribute("default", "");
    await expect(
      section.getByRole("link", { name: t("site.product.video.transcript", { product: name }) }),
    ).toHaveAttribute("href", `${MEDIA}/${id}/transcript.txt`);
    const sources = await section
      .locator("video source")
      .evaluateAll((els) => els.map((el) => el.getAttribute("src")));
    expect(sources).toEqual([`${MEDIA}/${id}/demo.webm`, `${MEDIA}/${id}/demo.mp4`]);
  }
});

test("a video that fails to load keeps the poster with the plain message, not a broken player", async ({
  page,
}) => {
  await page.route(`${MEDIA}/**`, (route) => route.abort());
  await page.setViewportSize(DESKTOP);
  await open(page, "/product");
  const section = page.locator("section#hub");
  await section.locator("video").evaluate((el: HTMLVideoElement) => el.load());
  const message = section.getByText(t("site.product.video.unavailable"));
  await expect(message).toBeVisible();
  await expect(section.locator("video")).toHaveCount(0);
  await expect(section.locator(".product-media")).toBeVisible();
  // The walkthrough is still there to read.
  await expect(section.locator(".product-step")).toHaveCount(5);
});

test("a browser that cannot play any source shows the message at once", async ({ page }) => {
  await page.addInitScript(() => {
    HTMLMediaElement.prototype.canPlayType = () => "";
  });
  await open(page, "/product");
  for (const { id } of PRODUCTS) {
    await expect(
      page.locator(`section#${id}`).getByText(t("site.product.video.unavailable")),
    ).toBeVisible();
    await expect(page.locator(`section#${id} video`)).toHaveCount(0);
  }
});

test("pressing play moves no focus and the controls are native and reachable by Tab", async ({
  page,
}) => {
  // The media origin never answers, so the clip neither plays nor fails while focus is checked.
  await page.route(/\.(webm|mp4)$/, () => {});
  await open(page, "/product");
  const video = page.locator("section#kiosk video");
  await video.scrollIntoViewIfNeeded();
  await video.focus();
  expect(await video.evaluate((el) => el === document.activeElement)).toBe(true);
  await page.keyboard.press("Space"); // the native play/pause key; the clip is still loading
  expect(await video.evaluate((el) => el === document.activeElement || !el.isConnected)).toBe(true);
  // Tab reaches the video before the transcript link.
  await page
    .locator("section#kiosk")
    .getByRole("link", { name: /transcript/ })
    .focus();
  await page.keyboard.press("Shift+Tab");
  await expect(video).toBeFocused();
});

test("the Console section says console time runs from the Hub tile, without a date", async ({
  page,
}) => {
  await open(page, "/product");
  const note = page.locator("section#console .product-note");
  await expect(note).toHaveText(t("site.product.console.note"));
  await expect(note).toContainText("Hub tile");
  await expect(note).toContainText("Raspberry Pi");
  expect(await note.innerText()).not.toMatch(/\b(20\d\d|Q[1-4]|soon|next (month|year)|by )\b/i);
});

test("the video frame stays dark in light mode and the steps stack at 390 px", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await open(page, "/product", "light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect(page.locator("section#hub .product-media")).toHaveAttribute("data-theme", "dark");
  const xs = await page
    .locator("section#hub .product-step")
    .evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().left)));
  expect(new Set(xs).size).toBe(1);

  await open(page, "/product", "hc");
  await expect(page.locator("section#hub .product-media")).toHaveAttribute("data-theme", "hc");
});

test("Get started closes the page", async ({ page }) => {
  await open(page, "/product");
  const band = page.locator(".product-band");
  await expect(band.getByRole("link", { name: t("site.cta.get_started") })).toHaveAttribute(
    "href",
    "https://start.arena360.cloud",
  );
});
