import { expect, test } from "@playwright/test";
import type { SessionListRow } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { CORS, HUB, device, json, snapshot } from "./floor-fixture";

const row = (id: string, extra: Partial<SessionListRow>): SessionListRow => ({
  id,
  device_id: device(1, {}).device_id,
  device: "PS5 1",
  guest_no: 1,
  mode: "postpaid",
  started_at: Date.now() - 3_600_000,
  ended_at: Date.now() - 600_000,
  state: "settled",
  ...extra,
});

test("the Sessions screen lists whether a console's controllers came back", async ({ page }) => {
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route) => {
    if (route.request().method() === "OPTIONS")
      return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(route.request().url()).pathname) {
      case "/ui/snapshot":
        return json(route, snapshot([]));
      case "/ui/me":
        return json(route, {
          staff_id: "S",
          display_name: "Counter",
          role_name: "Counter",
          permissions: ["session.start"],
        });
      case "/ui/settings":
        return json(route, []);
      case "/ui/sessions":
        return json(route, {
          business_day: "2026-10-28",
          sessions: [
            row("S1", { controller_check: "returned" }),
            row("S2", { controller_check: "missing" }),
            row("S3", {}),
          ],
        });
      default:
        return route.abort();
    }
  });
  await page.goto("/sessions");
  const cells = page.getByTestId("session-controllers");
  await expect(cells).toHaveText([
    en["hub.sessions.controllers.returned"],
    en["hub.sessions.controllers.missing"],
    "—",
  ]);
});

test("a settled, numbered bill on the Sessions screen offers Reprint and sends receipt.reprint", async ({
  page,
}) => {
  const sent: unknown[] = [];
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "tok"));
  await page.route(`${HUB}/**`, async (route) => {
    if (route.request().method() === "OPTIONS")
      return route.fulfill({ status: 204, headers: CORS });
    switch (new URL(route.request().url()).pathname) {
      case "/ui/snapshot":
        return json(route, snapshot([]));
      case "/ui/me":
        return json(route, {
          staff_id: "S",
          display_name: "Counter",
          role_name: "Counter",
          permissions: ["session.start", "pos.sell"],
        });
      case "/ui/settings":
        return json(route, []);
      case "/ui/sessions":
        return json(route, {
          business_day: "2026-10-28",
          sessions: [
            row("S1", { invoice_no: "DEV01/26-27/E1k/000001", receipt_order_id: "O1" }),
            row("S2", {}),
          ],
        });
      case "/ui/intent": {
        sent.push(route.request().postDataJSON().intent.payload);
        return json(route, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.abort();
    }
  });
  await page.goto("/sessions");
  const rows = page.getByTestId("session-row");
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(1).getByRole("button", { name: en["hub.sessions.reprint"] })).toHaveCount(
    0,
  );
  await rows.first().getByRole("button", { name: en["hub.sessions.reprint"] }).click();
  await expect(page.getByTestId("reprint-note")).toHaveText(en["hub.pos.recent.reprint_queued"]);
  expect(sent).toEqual([{ kind: "receipt.reprint", order_id: "O1" }]);
});
