import type { HubSnapshot, SessionStaff, SnapshotDevice } from "@arena/ui/generated";
import type { Route } from "@playwright/test";
import { ORIGIN } from "./origin";

export const HUB = "http://127.0.0.1:7409";
export const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
export const MANAGER: SessionStaff = {
  staff_id: "01JMANAGER0000000000000000",
  display_name: "Manager",
  role_name: "Manager",
  permissions: ["settings.edit"],
};

export const device = (n: number, extra: Partial<SnapshotDevice>): SnapshotDevice => ({
  device_id: `01JDEV${String(n).padStart(20, "0")}`,
  label: `PC ${n}`,
  class: "paired",
  zone: "Z1",
  online: true,
  kind: "pc",
  group_id: "G1",
  floor_x: (n - 1) % 6,
  floor_y: Math.floor((n - 1) / 6),
  status: "online",
  ...extra,
});

export const snapshot = (devices: SnapshotDevice[], rate: number | null = 10000): HubSnapshot => ({
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [
    {
      id: "G1",
      name: "Gaming",
      kind: "pc",
      colour: "teal",
      sort_order: 0,
      rev: 1,
      rate_minor: rate,
    },
  ],
  zones: [{ id: "Z1", name: "Main hall", sort_order: 0, rev: 1 }],
  devices,
  locale: {
    currency: "INR",
    exponent: 2,
    symbol: "₹",
    symbol_position: "before",
    number_format: "indian",
    tz: "Asia/Kolkata",
    day_start_min: 360,
    fy_start_month: 4,
    pending: null,
  },
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
});

export const json = (route: Route, body: unknown) =>
  route.fulfill({
    status: 200,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
