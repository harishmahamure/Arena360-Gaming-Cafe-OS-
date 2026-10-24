import { expect, test } from "@playwright/test";
import en from "../../../../packages/ui/src/messages/en.json" with { type: "json" };

// The Hub UI against the real Hub (no stubs), run by `scripts/wan-down/run.sh` inside a network
// namespace with no route out (and once with the network up). Every request must stay on
// loopback; with `ARENA_WAN_DOWN=1` the header must say the internet is offline.

const LOOPBACK = new Set(["localhost", "127.0.0.1", "[::1]"]);
const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env;
const WAN_DOWN = env?.ARENA_WAN_DOWN === "1";

test("sign-in, floor and settings work on the real Hub with nothing leaving loopback", async ({
  page,
}) => {
  const remote: string[] = [];
  page.on("request", (req) => {
    const url = new URL(req.url());
    if (url.protocol === "data:" || url.protocol === "blob:") return;
    if (!LOOPBACK.has(url.hostname)) remote.push(req.url());
  });

  await page.goto("/sign-in");
  await page.getByLabel(en["hub.sign_in.login"]).fill("manager");
  await page.getByLabel(en["hub.sign_in.password"], { exact: true }).fill("dev-manager-pass");
  await page.getByRole("button", { name: en["hub.sign_in.submit"] }).click();

  await expect(page).toHaveURL(/\/floor$/);
  await expect(page.getByTestId("hub-branch")).toHaveText("Dev Branch");
  await expect(page.getByTestId("hub-staff")).toContainText("Manager");
  await expect(page.getByText("Dev Kiosk")).toBeVisible();
  await expect(page.getByTestId("device-presence")).toHaveText(en["hub.floor.online"], {
    timeout: 15_000,
  });
  if (WAN_DOWN) {
    // The first probe runs at start; the chip names the link that is down.
    await expect(page.getByTestId("hub-chip-offline")).toContainText(
      en["hub.connectivity.internet_offline"],
      { timeout: 40_000 },
    );
  }

  await page.getByRole("link", { name: en["hub.nav.settings"] }).first().click();
  await expect(page).toHaveURL(/\/settings/);
  await expect(page.locator("h1")).toHaveCount(1);

  expect(remote).toEqual([]);
});
