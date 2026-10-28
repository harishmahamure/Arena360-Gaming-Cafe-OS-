import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";

// `GameMedia` in a real browser: the fallback chain, one muted looping video at a time, the
// pause control, the game-running unload, high contrast, and the content-hash cache.

const hash = (letter: string) => `/media/${letter.repeat(64)}`;
const bytesOf = (base64: string) => Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
// Playwright wants a node `Buffer`; this package has no node types, so it is reached by name.
const asBody = (bytes: Uint8Array) =>
  (globalThis as unknown as { Buffer: { from(b: Uint8Array): never } }).Buffer.from(bytes);
const PNG = bytesOf(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
);
const IMMUTABLE = { "cache-control": "public, max-age=31536000, immutable" };

/** A short VP8 clip recorded in the browser, so no binary lives in the repo. */
async function recordWebm(page: Page): Promise<Uint8Array> {
  const base64 = await page.evaluate(async () => {
    const canvas = document.createElement("canvas");
    canvas.width = 64;
    canvas.height = 36;
    const ctx = canvas.getContext("2d")!;
    const recorder = new MediaRecorder(canvas.captureStream(15), {
      mimeType: "video/webm;codecs=vp8",
    });
    const chunks: Blob[] = [];
    recorder.ondataavailable = (e) => chunks.push(e.data);
    const stopped = new Promise((resolve) => (recorder.onstop = resolve));
    recorder.start();
    let n = 0;
    const timer = setInterval(() => {
      ctx.fillStyle = `hsl(${n++ * 30}, 80%, 50%)`;
      ctx.fillRect(0, 0, 64, 36);
    }, 66);
    await new Promise((resolve) => setTimeout(resolve, 900));
    clearInterval(timer);
    recorder.stop();
    await stopped;
    const bytes = new Uint8Array(await new Blob(chunks).arrayBuffer());
    let binary = "";
    for (const b of bytes) binary += String.fromCharCode(b);
    return btoa(binary);
  });
  return bytesOf(base64);
}

type Answer = "video" | "png" | "garbage" | "missing";
/** Answers `/media/<letter × 64>` requests; returns the per-letter request counts. */
async function serve(page: Page, answers: Record<string, Answer>) {
  await page.goto("/game-media");
  const webm = await recordWebm(page);
  const hits: Record<string, number> = {};
  await page.route(/\/media\/([a-f])\1{63}$/, (route: Route) => {
    const letter = route.request().url().slice(-1);
    hits[letter] = (hits[letter] ?? 0) + 1;
    const answer = answers[letter] ?? "missing";
    if (answer === "missing") return route.fulfill({ status: 404, body: "" });
    if (answer === "garbage")
      return route.fulfill({ status: 200, contentType: "video/webm", body: "not media" });
    return route.fulfill({
      status: 200,
      headers: IMMUTABLE,
      contentType: answer === "video" ? "video/webm" : "image/png",
      body: asBody(answer === "video" ? webm : PNG),
    });
  });
  return hits;
}

const open = (page: Page, query: string) => page.goto(`/game-media?${query}`);
const media = (page: Page, i = 0) => page.getByTestId(`media-${i}`);

test("game-media: a playing video is muted, looping, local, under a scrim with copy in its zone", async ({
  page,
}) => {
  await serve(page, { a: "video", b: "png" });
  await open(page, "video=a&cover=b");
  const frame = media(page);
  await expect(frame).toHaveAttribute("data-playing", "true");
  await expect(frame).toHaveAttribute("data-stage", "video");
  const video = frame.locator("video");
  await expect(video).toHaveCount(1);
  expect(await video.evaluate((v: HTMLVideoElement) => [v.muted, v.loop, v.volume])).toEqual([
    true,
    true,
    0,
  ]);
  expect(await video.getAttribute("src")).toBe(hash("a"));
  await expect(frame.getByTestId("game-media-scrim")).toBeVisible();
  const box = (await frame.boundingBox())!;
  const copy = (await frame.getByTestId("game-media-copy").boundingBox())!;
  expect(copy.x).toBeGreaterThanOrEqual(box.x - 1);
  expect(copy.x + copy.width).toBeLessThanOrEqual(box.x + box.width * 0.45 + 1);
  expect(copy.y).toBeGreaterThanOrEqual(box.y + box.height * 0.5 - 1);
  expect(copy.y + copy.height).toBeLessThanOrEqual(box.y + box.height + 1);
});

test("game-media: the video control is a 44px Tab stop that pauses on the frame and plays again", async ({
  page,
}) => {
  await serve(page, { a: "video" });
  await open(page, "video=a");
  const control = page.getByTestId("video-control");
  await expect(control).toBeVisible();
  const box = (await control.boundingBox())!;
  expect(box.width).toBeGreaterThanOrEqual(44);
  expect(box.height).toBeGreaterThanOrEqual(44);
  await expect(control).toHaveAttribute("aria-label", "Pause video");
  for (let i = 0; i < 3 && !(await control.evaluate((el) => el === document.activeElement)); i++) {
    await page.keyboard.press("Tab");
  }
  await expect(control).toBeFocused();
  const video = media(page).locator("video");
  await page.keyboard.press("Enter");
  await expect(control).toHaveAttribute("aria-label", "Play video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.paused)).toBe(true);
  const at = await video.evaluate((v: HTMLVideoElement) => v.currentTime);
  await page.waitForTimeout(400);
  expect(await video.evaluate((v: HTMLVideoElement) => v.currentTime)).toBe(at);
  await page.keyboard.press("Space");
  await expect(control).toHaveAttribute("aria-label", "Pause video");
  await expect.poll(() => video.evaluate((v: HTMLVideoElement) => v.paused)).toBe(false);
});

test("game-media: a video that cannot be decoded falls to the cover with nothing visible", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await serve(page, { a: "garbage", b: "png" });
  await open(page, "video=a&cover=b");
  const frame = media(page);
  await expect(frame).toHaveAttribute("data-stage", "cover");
  await expect(frame.locator("video")).toHaveCount(0);
  await expect(frame.locator("img")).toHaveAttribute("src", hash("b"));
  await expect(page.getByTestId("video-control")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("game-media: cover, then icon, then solid, as each step fails", async ({ page }) => {
  await serve(page, { a: "missing", b: "missing", c: "png" });
  await open(page, "video=a&cover=b&icon=c");
  const frame = media(page);
  await expect(frame).toHaveAttribute("data-stage", "icon");
  await expect(frame.locator("img.icon")).toHaveAttribute("src", hash("c"));

  await serve(page, { a: "missing", b: "missing", c: "missing" });
  await open(page, "video=a&cover=b&icon=c");
  await expect(media(page)).toHaveAttribute("data-stage", "solid");
  await expect(media(page).locator("img, video")).toHaveCount(0);

  await open(page, "");
  await expect(media(page)).toHaveAttribute("data-stage", "solid");
  const bg = await media(page).evaluate((el) => getComputedStyle(el).backgroundColor);
  const pageBg = await page.locator("main").evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(bg).toBe(pageBg);
});

test("game-media: an internet URL is never requested", async ({ page }) => {
  await serve(page, { b: "png" });
  const outside: string[] = [];
  page.on("request", (r) => {
    if (new URL(r.url()).hostname === "cdn.example.com") outside.push(r.url());
  });
  await open(page, "external=https://cdn.example.com/trailer.mp4&cover=b");
  await expect(media(page)).toHaveAttribute("data-stage", "cover");
  await page.waitForTimeout(300);
  expect(outside).toEqual([]);
});

test("game-media: only one video plays on the screen at a time", async ({ page }) => {
  await serve(page, { a: "video", b: "png" });
  await open(page, "video=a&cover=b&count=2");
  await expect(page.locator("video")).toHaveCount(1);
  await expect(page.getByTestId("video-control")).toHaveCount(1);
  await expect(media(page, 1).locator("video")).toHaveCount(1);
  await expect(media(page, 0).locator("video")).toHaveCount(0);
  await expect(media(page, 0).locator("img")).toHaveCount(1);
});

test("game-media: a running game unloads the video and leaves the cover until it ends", async ({
  page,
}) => {
  await serve(page, { a: "video", b: "png" });
  await open(page, "video=a&cover=b");
  const frame = media(page);
  await expect(frame).toHaveAttribute("data-playing", "true");
  await page.getByRole("button", { name: "A game starts" }).click();
  await expect(frame.locator("video")).toHaveCount(0);
  await expect(frame).toHaveAttribute("data-stage", "cover");
  await expect(frame.locator("img")).toHaveCount(1);
  await expect(page.getByTestId("video-control")).toHaveCount(0);
  await page.getByRole("button", { name: "The game ends" }).click();
  await expect(frame).toHaveAttribute("data-playing", "true");
});

test("game-media: high contrast shows solid bg with no scrim, video or control", async ({
  page,
}) => {
  await serve(page, { a: "video", b: "png", c: "png" });
  await open(page, "theme=hc&video=a&cover=b&icon=c");
  const frame = media(page);
  await expect(frame).toHaveAttribute("data-stage", "solid");
  await expect(frame.locator("img, video")).toHaveCount(0);
  await expect(frame.getByTestId("game-media-scrim")).toHaveCount(0);
  await expect(page.getByTestId("video-control")).toHaveCount(0);
  await expect(frame.getByTestId("game-media-copy")).toBeVisible();
});

test("game-media: media shown again comes from the cache by content hash", async ({ page }) => {
  const hits = await serve(page, { b: "png" });
  await open(page, "cover=b");
  await expect(media(page).locator("img")).toHaveJSProperty("complete", true);
  expect(hits.b).toBe(1);
  await page.getByRole("button", { name: "Show view B" }).click();
  await expect(page.locator('[data-view="b"]')).toBeVisible();
  await expect(media(page).locator("img")).toHaveJSProperty("complete", true);
  await page.getByRole("button", { name: "Show view A" }).click();
  await expect(page.locator('[data-view="a"]')).toBeVisible();
  await expect(media(page).locator("img")).toHaveJSProperty("complete", true);
  expect(hits.b).toBe(1);
});

test("game-media: no element uses backdrop-filter", async ({ page }) => {
  await serve(page, { a: "video", b: "png" });
  await open(page, "video=a&cover=b");
  await expect(media(page)).toHaveAttribute("data-playing", "true");
  const blurred = await page.evaluate(
    () =>
      [...document.querySelectorAll("*")].filter((el) => {
        const s = getComputedStyle(el);
        return s.backdropFilter !== "none" && s.backdropFilter !== "";
      }).length,
  );
  expect(blurred).toBe(0);
});

for (const theme of ["dark", "hc"]) {
  test(`game-media: no axe violations with the cover and copy (${theme})`, async ({ page }) => {
    await serve(page, { b: "png" });
    await open(page, `theme=${theme}&cover=b`);
    await expect(media(page).locator(theme === "hc" ? "h2" : "img")).toBeVisible();
    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations.map((v) => v.id)).toEqual([]);
  });
}
