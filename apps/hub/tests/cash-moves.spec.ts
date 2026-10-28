import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { SnapshotShift } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { parseAmount } from "../src/lib/shift";
import { CORS, HUB, device, json as ok, snapshot } from "./floor-fixture";

// Deposits and expenses (Story 5.25) on the Shift screen. The Hub's loopback API is stubbed with
// `page.route`; the stub records every intent and the elevation tickets that ride them.

const PIN = "2222";
const OPEN: SnapshotShift = {
  id: "SH1",
  staff_id: "01JMANAGER0000000000000000",
  staff_name: "Ravi",
  opened_at: Date.UTC(2026, 9, 28, 9, 30),
  float_minor: 5000,
  events: 1,
};

type Sent = { payload: Record<string, unknown>; ticket?: string };
type Hub = {
  sent: Sent[];
  elevations: { items: unknown[]; pin: string }[];
  fail: { status: number; code: string; field?: string } | null;
};

async function open(page: Page, permissions: string[]): Promise<Hub> {
  const hub: Hub = { sent: [], elevations: [], fail: null };
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route: Route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot": {
        const snap = snapshot([device(1, {})]);
        snap.shift = { open: OPEN, carried_minor: 5000 };
        return ok(route, snap);
      }
      case "/ui/me":
        return ok(route, {
          staff_id: "01JCOUNTER0000000000000000",
          display_name: "Asha",
          role_name: "Counter",
          permissions,
          locked: false,
          has_pin: true,
        });
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n`,
        });
      case "/ui/elevate": {
        const body = req.postDataJSON() as { items: unknown[]; pin: string };
        hub.elevations.push(body);
        if (body.pin !== PIN) {
          return route.fulfill({
            status: 403,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ code: "PIN_REJECTED", message: "no" }),
          });
        }
        return ok(route, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Sent["payload"] };
          elevation_ticket?: string;
        };
        hub.sent.push({ payload: body.intent.payload, ticket: body.elevation_ticket });
        if (hub.fail) {
          return route.fulfill({
            status: hub.fail.status,
            contentType: "application/json",
            headers: CORS,
            body: JSON.stringify({ message: "no", ...hub.fail }),
          });
        }
        return ok(route, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/shift");
  await expect(page.getByTestId("cash-moves")).toBeVisible();
  return hub;
}

test("the amount is more than zero, in whole numbers or up to the currency's decimals", () => {
  expect(parseAmount("0", 2)).toBeNull();
  expect(parseAmount("0.00", 2)).toBeNull();
  expect(parseAmount("12.5", 2)).toBe(1250);
  expect(parseAmount("1.234", 2)).toBeNull();
  expect(parseAmount("-5", 2)).toBeNull();
  expect(parseAmount("", 2)).toBeNull();
});

test("a deposit needs a slip reference and sends the amount in minor units", async ({ page }) => {
  const hub = await open(page, ["cash.manage"]);
  await page.getByRole("button", { name: en["hub.shift.cash.deposit"] }).click();
  await page.getByLabel(en["hub.shift.cash.amount"]).fill("40");
  await page.getByRole("button", { name: en["hub.shift.cash.save"] }).click();
  await expect(page.getByText(en["hub.shift.cash.err.reference"])).toBeVisible();
  expect(hub.sent).toHaveLength(0);
  await page.getByLabel(en["hub.shift.cash.reference"]).fill("SLIP-9");
  await page.getByRole("button", { name: en["hub.shift.cash.save"] }).click();
  await expect(page.getByTestId("cash-done")).toHaveText(en["hub.shift.cash.deposited"]);
  expect(hub.sent).toEqual([
    { payload: { kind: "cash.deposit", amount_minor: 4000, reference: "SLIP-9" } },
  ]);
});

test("a Counter deposit asks a manager's PIN in place and the ticket rides the intent", async ({
  page,
}) => {
  const hub = await open(page, []);
  await page.getByRole("button", { name: en["hub.shift.cash.deposit"] }).click();
  await page.getByLabel(en["hub.shift.cash.amount"]).fill("10");
  await page.getByLabel(en["hub.shift.cash.reference"]).fill("S1");
  await page.getByRole("button", { name: en["hub.shift.cash.save"] }).click();
  await expect(page.getByRole("group", { name: en["hub.shift.cash.pin_deposit"] })).toBeVisible();
  expect(hub.sent).toHaveLength(0);
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(page.getByText(en["ui.pin.rejected"])).toBeVisible();
  await page.keyboard.type(PIN);
  await page.keyboard.press("Enter");
  await expect.poll(() => hub.sent.length).toBe(1);
  expect(hub.elevations.at(-1)?.items).toEqual([{ intent_kind: "cash.deposit" }]);
  expect(hub.sent[0].ticket).toBe("T1");
});

test("an expense carries its category, who paid and a receipt photo of at most 200 KB", async ({
  page,
}) => {
  const hub = await open(page, ["cash.manage"]);
  await page.getByRole("button", { name: en["hub.shift.cash.expense"] }).click();
  await page.getByLabel(en["hub.shift.cash.category"]).selectOption("food_stock");
  await page.getByLabel(en["hub.shift.cash.amount"]).fill("25.50");
  await page.getByLabel(en["hub.shift.cash.paid_from"]).selectOption("owner");
  // A large, noisy picture the page has to shrink, handed to the file input as a user would.
  await page.evaluate(async () => {
    const canvas = document.createElement("canvas");
    canvas.width = 3000;
    canvas.height = 3000;
    const ctx = canvas.getContext("2d")!;
    const image = ctx.createImageData(3000, 3000);
    for (let i = 0; i < image.data.length; i += 4) {
      image.data[i] = (i * 7) % 256;
      image.data[i + 1] = (i * 13) % 256;
      image.data[i + 2] = (i * 29) % 256;
      image.data[i + 3] = 255;
    }
    ctx.putImageData(image, 0, 0);
    const blob = await new Promise<Blob>((r) => canvas.toBlob((b) => r(b!), "image/png"));
    const transfer = new DataTransfer();
    transfer.items.add(new File([blob], "receipt.png", { type: "image/png" }));
    const input = document.querySelector<HTMLInputElement>('[data-testid="cash-photo"]')!;
    input.files = transfer.files;
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await expect(page.getByTestId("cash-photo-ready")).toBeVisible();
  await page.getByRole("button", { name: en["hub.shift.cash.save"] }).click();
  await expect(page.getByTestId("cash-done")).toHaveText(en["hub.shift.cash.spent"]);
  expect(hub.sent).toHaveLength(1);
  const sent = hub.sent[0].payload as { receipt_photo?: string } & Record<string, unknown>;
  expect(sent).toMatchObject({
    kind: "expense.create",
    category: "food_stock",
    amount_minor: 2550,
    paid_from: "owner",
  });
  const photo = sent.receipt_photo ?? "";
  expect(photo.startsWith("data:image/jpeg;base64,")).toBe(true);
  expect(Math.floor(((photo.length - photo.indexOf(",") - 1) * 3) / 4)).toBeLessThanOrEqual(
    200 * 1024,
  );
});

test("a refused deposit shows the drawer-short message and nothing about the amount", async ({
  page,
}) => {
  const hub = await open(page, ["cash.manage"]);
  hub.fail = { status: 409, code: "DRAWER_SHORT", field: "amount_minor" };
  await page.getByRole("button", { name: en["hub.shift.cash.deposit"] }).click();
  await page.getByLabel(en["hub.shift.cash.amount"]).fill("999");
  await page.getByLabel(en["hub.shift.cash.reference"]).fill("S1");
  await page.getByRole("button", { name: en["hub.shift.cash.save"] }).click();
  await expect(page.getByTestId("cash-error")).toHaveText(en["hub.shift.cash.err.short"]);
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => v.id)).toEqual([]);
});
