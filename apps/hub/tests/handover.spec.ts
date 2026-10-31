import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { HandoverView } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, MANAGER, device, json as ok, snapshot } from "./floor-fixture";

const json = (route: Parameters<typeof ok>[0], body: unknown, status = 200) =>
  status === 200
    ? ok(route, body)
    : route.fulfill({
        status,
        contentType: "application/json",
        headers: CORS,
        body: JSON.stringify(body),
      });

// The Android Hub hands the branch to a Windows Hub (Story 10.17): the manager opens the "Move this
// branch" item, confirms with a PIN and reads the 6-digit code; once a PC claims it the tablet stops
// and says so. The Windows Hub's waiting screen takes the tablet's address and the code. The Hub's
// loopback API is stubbed with `page.route`.

const PC = "Front desk";

type Stub = {
  view: HandoverView | null;
  moveNotice: boolean;
  pins: string[];
  steps: string[];
  refuse: { code: string; field?: string } | null;
};

async function android(page: Page, view: HandoverView | null = null): Promise<Stub> {
  const hub: Stub = { view, moveNotice: true, pins: [], steps: [], refuse: null };
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "manager-token"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const path = new URL(req.url()).pathname;
    switch (path) {
      case "/ui/snapshot": {
        const snap = snapshot([device(1, {})]);
        if (hub.moveNotice) snap.move_notice = { id: "M1", target_hub_name: PC };
        if (hub.view) snap.handover = hub.view;
        snap.lan = {
          addresses: ["192.168.1.20"],
          lan_port: 7400,
          web_port: 7401,
          fingerprint: "ab".repeat(32),
          no_lan: false,
        };
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, { ...MANAGER, locked: false, has_pin: true });
      case "/ui/settings":
        return json(route, []);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/handover/start":
      case "/ui/handover/file":
      case "/ui/handover/cancel": {
        const step = path.split("/").pop() as string;
        const body = req.postDataJSON() as { pin: string };
        hub.pins.push(body.pin);
        if (body.pin !== "1234") {
          return json(route, { code: "PIN_REJECTED", message: "wrong" }, 403);
        }
        if (hub.refuse) {
          return json(route, { ...hub.refuse, message: "refused" }, 400);
        }
        hub.steps.push(step);
        if (step === "start") {
          hub.view = { phase: "code", target_hub_name: PC, code: "482913" };
        } else if (step === "file") {
          hub.view = {
            phase: "file",
            target_hub_name: PC,
            last_seq: 77,
            file_name: "DEV01-2027-01-15-1330-move.a360",
          };
        } else {
          hub.view = null;
          hub.moveNotice = true;
        }
        return json(route, hub.view);
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  // A tablet that has stopped shows no floor.
  await expect(
    view?.phase === "transferring" || view?.phase === "file" || view?.phase === "moved"
      ? page.getByTestId("hub-handover-stopped")
      : page.locator("[data-component=floor-tile]").first(),
  ).toBeVisible();
  return hub;
}

const pinIn = async (page: Page, digits: string) => {
  const pin = page.getByTestId("handover-approval");
  await expect(pin).toBeVisible();
  for (const d of digits) await pin.getByRole("button", { name: d, exact: true }).click();
  await pin.getByRole("button", { name: en["ui.pin.key_submit"] }).click();
};

async function openPanel(page: Page) {
  await page.getByRole("button", { name: new RegExp(en["hub.alerts.button"]) }).click();
  await page.getByTestId("alert-row").getByRole("button").click();
  await expect(page.getByTestId("move-branch")).toBeVisible();
}

test("a manager confirms the move with a PIN and the tablet shows a 6-digit code and its address", async ({
  page,
}) => {
  const hub = await android(page);
  await openPanel(page);
  const panel = page.getByTestId("move-branch");
  await expect(panel).toContainText(`Move this branch to ${PC}`);
  await expect(page.getByTestId("handover-code")).toHaveCount(0);
  await panel.getByRole("button", { name: en["hub.handover.show_code"] }).click();
  // A wrong PIN is refused in place and nothing starts.
  await pinIn(page, "9999");
  await expect(page.getByTestId("handover-approval")).toContainText(en["ui.pin.rejected"]);
  expect(hub.steps).toEqual([]);
  await page.keyboard.press("Escape");
  await panel.getByRole("button", { name: en["hub.handover.show_code"] }).click();
  await pinIn(page, "1234");
  await expect(page.getByTestId("handover-code")).toHaveText("482913");
  await expect(page.getByTestId("handover-address")).toContainText("192.168.1.20");
  expect(hub.steps).toEqual(["start"]);
});

test("cancelling the move from the code screen asks for the PIN and closes the panel", async ({
  page,
}) => {
  const hub = await android(page, { phase: "code", target_hub_name: PC, code: "482913" });
  await openPanel(page);
  await page
    .getByTestId("move-branch")
    .getByRole("button", { name: en["hub.handover.cancel_move"] })
    .click();
  await pinIn(page, "1234");
  await expect(page.getByTestId("move-branch")).toHaveCount(0);
  expect(hub.steps).toEqual(["cancel"]);
});

test("a tablet whose code was claimed shows that it stopped, and Cancel move needs the PIN", async ({
  page,
}) => {
  const hub = await android(page, {
    phase: "transferring",
    target_hub_name: PC,
    last_seq: 77,
  });
  const screen = page.getByTestId("hub-handover-stopped");
  await expect(screen).toBeVisible();
  await expect(page.locator("h1")).toHaveText(`Moving this branch to ${PC}`);
  await expect(screen).toContainText("handing everything to " + PC);
  await expect(screen).toContainText("Last change: 77");
  // Nothing else is shown: no rail, no floor.
  await expect(page.locator("nav")).toHaveCount(0);
  await screen.getByRole("button", { name: en["hub.handover.cancel_move"] }).click();
  await pinIn(page, "1234");
  await expect(page.getByTestId("hub-handover-stopped")).toHaveCount(0);
  expect(hub.steps).toEqual(["cancel"]);
});

test("Use a file instead writes the snapshot and names the file", async ({ page }) => {
  const hub = await android(page, { phase: "transferring", target_hub_name: PC });
  await page
    .getByTestId("hub-handover-stopped")
    .getByRole("button", { name: en["hub.handover.use_file"] })
    .click();
  await pinIn(page, "1234");
  await expect(page.getByTestId("handover-file")).toContainText("DEV01-2027-01-15-1330-move.a360");
  expect(hub.steps).toEqual(["file"]);
});

test("a missing backup folder is said in words", async ({ page }) => {
  const hub = await android(page, { phase: "transferring", target_hub_name: PC });
  hub.refuse = { code: "VALIDATION_FAILED", field: "folder" };
  await page
    .getByTestId("hub-handover-stopped")
    .getByRole("button", { name: en["hub.handover.use_file"] })
    .click();
  await pinIn(page, "1234");
  await expect(page.getByTestId("handover-problem")).toContainText(en["hub.handover.error.folder"]);
});

test("a tablet that handed over only says the branch has moved", async ({ page }) => {
  await android(page, { phase: "moved", target_hub_name: PC, last_seq: 77 });
  await expect(page.locator("h1")).toHaveText(`This branch has moved to ${PC}`);
  await expect(page.getByRole("button")).toHaveCount(0);
});

test("the stopped screen meets the accessibility floor", async ({ page }) => {
  await android(page, { phase: "transferring", target_hub_name: PC });
  await expect(page.getByTestId("hub-handover-stopped")).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});

// ---- The Windows Hub's waiting screen ----

type Pc = { joins: { address: string; code: string }[]; answer: "ok" | "refused" | "down" };

async function windows(page: Page, received: HandoverView | null = null): Promise<Pc> {
  const pc: Pc = { joins: [], answer: "ok" };
  let view = received;
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([]);
        snap.paired = true;
        snap.migration_wait = "Pixel Arena Gaming";
        if (view) snap.handover = view;
        return json(route, snap);
      }
      case "/ui/me":
        return json(route, { code: "AUTH_FAILED", message: "sign in" }, 401);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/migration/join": {
        pc.joins.push(req.postDataJSON() as { address: string; code: string });
        if (pc.answer === "down") return route.abort();
        if (pc.answer === "refused") {
          return json(
            route,
            { code: "PAIR_CODE_INVALID", message: "That code is not right." },
            401,
          );
        }
        view = { phase: "received", target_hub_name: "Pixel Arena Gaming", last_seq: 77 };
        return json(route, view);
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  await expect(page.getByTestId("hub-migration-wait")).toBeVisible();
  return pc;
}

const address = (page: Page) => page.getByTestId("handover-address-input");
const code = (page: Page) => page.getByTestId("handover-code-input");
const join = (page: Page) => page.getByRole("button", { name: en["hub.migration_wait.join"] });

test("the PC takes the tablet's address and the 6-digit code and says when the branch arrived", async ({
  page,
}) => {
  const pc = await windows(page);
  await expect(join(page)).toBeDisabled();
  await address(page).fill("192.168.1.20");
  await code(page).fill("4829");
  await expect(join(page)).toBeDisabled();
  await code(page).fill("482913");
  await join(page).click();
  await expect(page.getByTestId("handover-received")).toContainText("last change 77");
  expect(pc.joins).toEqual([{ address: "192.168.1.20", code: "482913" }]);
  await expect(page.getByTestId("handover-join")).toHaveCount(0);
});

test("a refused code is shown in the Hub's words and the form stays", async ({ page }) => {
  const pc = await windows(page);
  pc.answer = "refused";
  await address(page).fill("192.168.1.20");
  await code(page).fill("000000");
  await join(page).click();
  await expect(page.getByTestId("handover-problem")).toContainText("That code is not right.");
  await expect(page.getByTestId("handover-join")).toBeVisible();
});

test("an unreachable Hub service is said plainly", async ({ page }) => {
  const pc = await windows(page);
  pc.answer = "down";
  await address(page).fill("192.168.1.20");
  await code(page).fill("482913");
  await join(page).click();
  await expect(page.getByTestId("handover-problem")).toContainText(
    en["hub.migration_wait.unreachable"],
  );
});

test("a PC that already holds the snapshot shows it after a reload", async ({ page }) => {
  await windows(page, { phase: "received", target_hub_name: "Pixel Arena Gaming", last_seq: 5 });
  await expect(page.getByTestId("handover-received")).toContainText("last change 5");
});
