import type { Page } from "@playwright/test";
import type { OwnerBranch } from "@arena/ui/generated";
import { t } from "../../../packages/ui/src/messages/index.js";

export { t };

export type Branch = OwnerBranch;

const base = {
  timezone: "Asia/Kolkata",
  live: false,
  hub: { paired: false },
  waiting: false,
} as const;

export const PAID: Branch[] = [
  {
    ...base,
    id: "b1",
    name: "Pixel Arena MG Road",
    branch_code: "MGR",
    tier: "basic",
    flags: ["tunnel"],
  },
  {
    ...base,
    id: "b2",
    name: "Pixel Arena Indiranagar",
    branch_code: "IND",
    tier: "free",
    flags: [],
  },
];
export const FREE: Branch[] = [
  { ...base, id: "b1", name: "Pixel Arena MG Road", branch_code: "MGR", tier: "free", flags: [] },
];

export async function stubCloud(page: Page, branches: Branch[] = PAID) {
  const calls: string[] = [];
  await page.route("**/v1/**", (route) => {
    const req = route.request();
    const path = new URL(req.url()).pathname;
    calls.push(`${req.method()} ${path}`);
    const json = (body: unknown, status = 200) =>
      route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
    if (path === "/v1/session")
      return json({
        owner: {
          name: "Asha",
          cafe_name: "Pixel Arena Gaming",
          city: "Bengaluru",
          mfa_state: "enrolled",
          has_password: true,
          backup_codes_acked: true,
          sms_fallback: false,
        },
      });
    if (path === "/v1/branches" && req.method() === "GET") return json(branches);
    if (path === "/v1/branches" && req.method() === "POST")
      return json({ ...branches[0], id: "b9", name: req.postDataJSON().name });
    if (path.startsWith("/v1/branches/") && req.method() === "PATCH")
      return json({ ...branches[0], ...req.postDataJSON() });
    if (path === "/v1/meta/timezones") return json(["Asia/Dubai", "Asia/Kolkata"]);
    if (path === "/v1/wizard/install-hub/code")
      return json({ code: "ABCD2345", expires_at: Date.now() + 900_000, server_now: Date.now() });
    if (path === "/v1/signout") return json(null, 200);
    return json({ code: "NOT_FOUND", message: "" }, 404);
  });
  return calls;
}

export const cacheCount = (page: Page) =>
  page.evaluate(
    () =>
      new Promise<number>((ok) => {
        const req = indexedDB.open("arena-owner", 1);
        req.onupgradeneeded = () => req.transaction!.abort();
        req.onerror = () => ok(-1);
        req.onsuccess = () => {
          const c = req.result.transaction("payloads").objectStore("payloads").count();
          c.onsuccess = () => ok(c.result);
        };
      }),
  );
