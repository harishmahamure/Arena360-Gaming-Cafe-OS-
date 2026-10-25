import { expect, test, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// The floor's Online / Offline follows the LAN: `event: devices.changed` on `/ui/events` makes
// the Hub UI fetch the snapshot again. The Hub's loopback API is stubbed with `page.route`; the
// stream asks for a long reconnect delay, so the refetch can only come from the event.

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const MANAGER: SessionStaff = {
  staff_id: "01JMANAGER0000000000000000",
  display_name: "Manager",
  role_name: "Manager",
  permissions: ["settings.edit"],
};
const LOCALE: HubSnapshot["locale"] = {
  currency: "INR",
  exponent: 2,
  symbol: "₹",
  symbol_position: "before",
  number_format: "indian",
  tz: "Asia/Kolkata",
  day_start_min: 360,
  fy_start_month: 4,
  pending: null,
};

const snapshot = (online: boolean): HubSnapshot => ({
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [],
  zones: [],
  devices: [
    {
      device_id: "01JDEVKIOSK0000000000000000",
      label: "Dev Kiosk",
      class: "paired",
      zone: null,
      online,
    },
  ],
  locale: LOCALE,
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
});

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

test("devices.changed on the event stream refetches the snapshot", async ({ page }) => {
  let online = false;
  let snapshots = 0;
  let streamed = false;
  await page.addInitScript(() => sessionStorage.setItem("arena.hub.token", "manager-token"));
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    switch (url.pathname) {
      case "/ui/snapshot":
        snapshots++;
        return json(route, 200, snapshot(online));
      case "/ui/me":
        return json(route, 200, MANAGER);
      case "/ui/settings":
        return json(route, 200, []);
      case "/ui/events": {
        if (streamed) return route.abort();
        streamed = true;
        // The floor has drawn the offline snapshot first; then the kiosk comes online.
        while (snapshots < 1) await new Promise((r) => setTimeout(r, 20));
        online = true;
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 600000\n\nevent: devices.changed\ndata: {}\n\n",
        });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/floor");
  const presence = page.getByTestId("device-presence");
  await expect(presence).toHaveText(en["hub.floor.online"]);
  expect(streamed).toBe(true);
  expect(snapshots).toBeGreaterThanOrEqual(2);
});
