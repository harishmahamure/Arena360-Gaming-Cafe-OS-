import { expect, test } from "@playwright/test";
import { PAID, stubCloud, t } from "./stub";

// Story 8.42: Today's first render on the reference phone profile, "a 3-year-old phone over 4G"
// (NFR7). The `reference-phone` project is the only one that runs this file: a Moto G4 with the
// CPU slowed 4x (Lighthouse's mobile setting), a cold cache, and a link of `OWNER_DOWN_MBIT`
// (9) down, `OWNER_UP_MBIT` (1.5) up and `OWNER_RTT_MS` (100) round trip. The cloud answers are
// the stub's, each held for that round trip plus `OWNER_SERVER_MS` (130 ms) in the cloud. The
// time from the first request to the figures on the screen is written as the `first-render-ms`
// annotation and checked against `OWNER_FIRST_RENDER_MS` (2 s). The release bench also runs the
// slower links (`OWNER_RTT_MS=170`, `OWNER_DOWN_MBIT=1.6`) for the record, see
// docs/runbooks/remote-bench.md.
const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env;
const num = (name: string, fallback: number) => Number(env?.[name] ?? fallback);
const budget = num("OWNER_FIRST_RENDER_MS", 2000);
const rtt = num("OWNER_RTT_MS", 100);
const apiMs = rtt + num("OWNER_SERVER_MS", 130);

test("Today's figures are on a reference phone in under two seconds", async ({ page }, info) => {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Network.enable");
  await cdp.send("Network.emulateNetworkConditions", {
    offline: false,
    downloadThroughput: (num("OWNER_DOWN_MBIT", 9) * 1024 * 1024) / 8,
    uploadThroughput: (num("OWNER_UP_MBIT", 1.5) * 1024 * 1024) / 8,
    latency: rtt,
  });
  await cdp.send("Emulation.setCPUThrottlingRate", { rate: 4 });

  await stubCloud(page, [PAID[0]]);
  // Registered last, so it runs first: hold each cloud answer, then hand it to the stub.
  await page.route("**/v1/**", async (route) => {
    await new Promise((done) => setTimeout(done, apiMs));
    await route.fallback();
  });

  const started = Date.now();
  await page.goto("/");
  const c = page.getByRole("region", { name: PAID[0].name });
  await expect(c.getByText("₹1,25,000.50")).toBeVisible({ timeout: 15_000 });
  const ms = Date.now() - started;
  info.annotations.push({ type: "first-render-ms", description: String(ms) });
  info.annotations.push({ type: "profile", description: `rtt ${rtt} ms, api ${apiMs} ms` });
  await expect(c.getByText(t("owner.today.open"), { exact: true })).toBeVisible();
  expect(ms, `first render took ${ms} ms`).toBeLessThan(budget);
});
