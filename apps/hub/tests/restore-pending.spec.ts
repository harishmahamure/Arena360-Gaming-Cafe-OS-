import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// A replacement Hub (Story 9.9): paired with a branch that already has history, it shows
// "Restore this branch" and nothing else, with the one option off until the owner releases the
// branch key. The Hub's loopback API is stubbed with `page.route`.

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};

function snapshot(released: boolean): HubSnapshot {
  return {
    branch: null,
    devices: [],
    locale: null,
    ui: { idle_lock_minutes: 5, hc: false },
    connectivity: { state: "online", checked_at: 1_800_000_000_000 },
    paired: true,
    restore_pending: "Pixel Arena Gaming",
    ...(released ? { restore_key_released: true } : {}),
  } as unknown as HubSnapshot;
}

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

async function open(page: Page, released = false) {
  const asked: string[] = [];
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    asked.push(url.pathname);
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot(released));
      case "/ui/me":
        return json(route, 401, { code: "AUTH_FAILED", message: "sign in" });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  await expect(page.getByTestId("hub-restore-pending")).toBeVisible();
  return asked;
}

test("a replacement Hub shows only Restore this branch with the option off and the reason shown", async ({
  page,
}) => {
  await open(page);
  await expect(page.locator("h1")).toHaveText(en["hub.restore_pending.title"]);
  await expect(page.getByTestId("restore-branch")).toContainText("Pixel Arena Gaming");
  const screen = page.getByTestId("hub-restore-pending");
  await expect(screen).toContainText(en["hub.restore_pending.replaces"]);
  await expect(screen).toContainText(en["hub.restore_pending.serves_nothing"]);
  const option = screen.getByRole("button", { name: en["hub.restore_pending.option"] });
  await expect(option).toBeDisabled();
  await expect(screen).toContainText(en["hub.restore_pending.waiting"]);

  // No sign-in screen, no rail, no floor: the Hub serves nothing until it is restored.
  await expect(page).toHaveURL(/\/floor$/);
  await expect(page.locator("nav")).toHaveCount(0);
  await expect(page.locator("h1")).toHaveCount(1);
});

test("the Restore this branch screen meets the accessibility floor", async ({ page }) => {
  await open(page);
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});

test("once the owner releases the branch key the option is on and the waiting copy is gone", async ({
  page,
}) => {
  await open(page, true);
  const screen = page.getByTestId("hub-restore-pending");
  await expect(screen.getByTestId("restore-released")).toHaveText(
    en["hub.restore_pending.released"],
  );
  const option = screen.getByRole("button", { name: en["hub.restore_pending.option"] });
  await expect(option).toBeEnabled();
  await expect(screen).not.toContainText(en["hub.restore_pending.waiting"]);
  // Still nothing else: restoring is a later step.
  await expect(page.locator("nav")).toHaveCount(0);
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});

// ---- Story 9.12: Restore from cloud backup ----

type CloudView = Record<string, unknown> | null;

const DOWNLOADING = {
  phase: "downloading",
  done: 3,
  total: 41,
  bytes: 2_500_000,
  percent: 7,
};
const GAP = {
  phase: "gap",
  done: 2,
  total: 41,
  bytes: 1000,
  percent: 4,
  gap: { epoch: "3:H", from_seq: 6, to_seq: 10, last_good_seq: 5, lost: 37 },
};
const REPLAYING = { phase: "replaying", done: 2, total: 9, percent: 22 };
const CHECKING = { phase: "checking", done: 3, total: 4, percent: 75 };
const READY = {
  phase: "ready",
  done: 4,
  total: 4,
  bytes: 5000,
  percent: 100,
  ready: { snapshot_date: "2026-10-29", batches: 1, last_seq: 5, partial: true },
};

/** The Hub's loopback API with the cloud-restore routes stubbed: `view` is what GET answers. */
async function openCloud(
  page: Page,
  tier: string,
  start: { view: CloudView },
  rebuild: { view: CloudView; after?: CloudView } = { view: null },
) {
  const posts: string[] = [];
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, { ...snapshot(true), tier });
      case "/ui/me":
        return json(route, 401, { code: "AUTH_FAILED", message: "sign in" });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/restore/cloud":
        if (req.method() === "POST") {
          posts.push("start");
          start.view = DOWNLOADING;
        }
        return json(route, 200, start.view);
      case "/ui/restore/cloud/cancel":
        posts.push("cancel");
        start.view = null;
        return json(route, 200, null);
      case "/ui/restore/cloud/rebuild":
        if (req.method() === "POST") {
          posts.push("rebuild");
          rebuild.view = REPLAYING;
          return json(route, 200, rebuild.view);
        }
        {
          const now = rebuild.view;
          // The next poll sees the rebuild one step on.
          if (now && rebuild.after) rebuild.view = rebuild.after;
          return json(route, 200, now);
        }
      case "/ui/restore/cloud/rebuild/cancel":
        posts.push("rebuild-cancel");
        rebuild.view = null;
        return json(route, 200, null);
      case "/ui/restore/cloud/partial":
        posts.push("partial");
        start.view = READY;
        return json(route, 200, READY);
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  await expect(page.getByTestId("hub-restore-pending")).toBeVisible();
  return posts;
}

test("on Free the cloud backup option is not shown", async ({ page }) => {
  await openCloud(page, "free", { view: null });
  const screen = page.getByTestId("hub-restore-pending");
  await expect(
    screen.getByRole("button", { name: en["hub.restore_pending.option"] }),
  ).toBeVisible();
  await expect(
    screen.getByRole("button", { name: en["hub.restore_pending.cloud_option"] }),
  ).toHaveCount(0);
});

test("a paid Hub with the key released confirms, downloads with progress and can cancel", async ({
  page,
}) => {
  const posts = await openCloud(page, "basic", { view: null });
  const screen = page.getByTestId("hub-restore-pending");
  const option = screen.getByRole("button", { name: en["hub.restore_pending.cloud_option"] });
  await expect(option).toBeEnabled();
  await option.click();
  await expect(screen.getByTestId("cloud-confirm")).toContainText("Pixel Arena Gaming");
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);

  await screen.getByRole("button", { name: en["hub.restore_pending.cloud_download"] }).click();
  const progress = screen.getByTestId("cloud-progress");
  await expect(progress).toContainText("Downloading 3 of 41");
  await expect(progress).toContainText("7%");
  await expect(progress.getByRole("progressbar")).toHaveAttribute("value", "7");
  expect(posts).toEqual(["start"]);

  await progress.getByRole("button", { name: en["hub.restore_pending.cloud_cancel"] }).click();
  await expect(screen.getByTestId("cloud-progress")).toHaveCount(0);
  await expect(option).toBeVisible();
  expect(posts).toEqual(["start", "cancel"]);
  // Still nothing else: the branch is not restored.
  await expect(page.locator("nav")).toHaveCount(0);
});

test("a gap names the range, the cost and offers to stop before it", async ({ page }) => {
  const posts = await openCloud(page, "basic", { view: GAP });
  const screen = page.getByTestId("hub-restore-pending");
  const gap = screen.getByTestId("cloud-gap");
  await expect(gap).toContainText("Changes 6 to 10 are missing or damaged.");
  await expect(gap).toContainText("up to change 5 would lose 37 changes");
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
  await gap.getByRole("button", { name: en["hub.restore_pending.cloud_gap_partial"] }).click();
  const ready = screen.getByTestId("cloud-ready");
  await expect(ready).toContainText("snapshot of 2026-10-29");
  await expect(ready).toContainText(en["hub.restore_pending.cloud_ready_partial"]);
  expect(posts).toEqual(["partial"]);
});

test("a complete download offers to rebuild the branch, shows Replaying then Checking and can cancel", async ({
  page,
}) => {
  const rebuild = { view: null as CloudView, after: CHECKING as CloudView };
  const posts = await openCloud(page, "basic", { view: READY }, rebuild);
  const screen = page.getByTestId("hub-restore-pending");
  await screen.getByRole("button", { name: en["hub.restore_pending.rebuild_option"] }).click();
  const confirm = screen.getByTestId("rebuild-confirm");
  await expect(confirm).toContainText(en["hub.restore_pending.rebuild_confirm"]);
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);

  await confirm.getByRole("button", { name: en["hub.restore_pending.rebuild_go"] }).click();
  const progress = screen.getByTestId("rebuild-progress");
  await expect(progress.getByTestId("rebuild-phase")).toContainText("Replaying 2 of 9");
  await expect(progress).toContainText("22%");
  expect(posts).toContain("rebuild");
  // The next poll shows the checks.
  await expect(progress.getByTestId("rebuild-phase")).toContainText("Checking 3 of 4");
  await expect(progress.getByRole("progressbar")).toHaveAttribute("value", "75");

  await progress.getByRole("button", { name: en["hub.restore_pending.rebuild_cancel"] }).click();
  await expect(screen.getByTestId("rebuild-progress")).toHaveCount(0);
  expect(posts).toContain("rebuild-cancel");
  // Back in "Restore this branch" with the downloaded backup still offered.
  await expect(screen.getByTestId("cloud-ready")).toBeVisible();
  await expect(page.locator("nav")).toHaveCount(0);
});

test("a rebuild that fails says why and keeps the downloaded backup", async ({ page }) => {
  const rebuild = {
    view: { phase: "failed", done: 1, total: 9, percent: 11, error: "RESTORE_CORRUPT" },
  };
  await openCloud(page, "basic", { view: READY }, rebuild);
  const screen = page.getByTestId("hub-restore-pending");
  const failed = screen.getByTestId("rebuild-failed");
  await expect(failed).toContainText("RESTORE_CORRUPT");
  await expect(
    failed.getByRole("button", { name: en["hub.restore_pending.rebuild_retry"] }),
  ).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});
