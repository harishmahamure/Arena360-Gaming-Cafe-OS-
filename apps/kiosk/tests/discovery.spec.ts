import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { DiscoveredHub } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

type Answer = { ok: boolean; code?: string; body: unknown };

const desk: DiscoveredHub = {
  addr: "192.168.1.10:7400",
  branch_name: "Koramangala",
  hub_name: "front-desk",
  fingerprint: "ab".repeat(32),
  protocol_version: 1,
  compatible: true,
  web_port: 7401,
};

const old: DiscoveredHub = {
  addr: "192.168.1.11:7400",
  branch_name: "Indiranagar",
  hub_name: "back-office",
  fingerprint: "cd".repeat(32),
  protocol_version: 9,
  compatible: false,
  web_port: 7401,
};

async function openUnpaired(page: Page, probes: Answer[] = []) {
  await page.addInitScript(
    ([commands, queued]: [typeof names.commands, Answer[]]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const calls: Array<{ cmd: string; args: unknown }> = [];
      const w = window as unknown as Record<string, unknown>;
      w.__calls = calls;
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event: string, id: number) => {
          listeners[event] = (listeners[event] ?? []).filter((x) => x !== id);
        },
      };
      w.__TAURI_INTERNALS__ = {
        transformCallback: (cb: (e: unknown) => void) => {
          const id = next++;
          callbacks.set(id, cb);
          return id;
        },
        invoke: async (cmd: string, args: Record<string, unknown>) => {
          switch (cmd) {
            case "plugin:event|listen":
              (listeners[args.event as string] ??= []).push(args.handler as number);
              return args.handler;
            case "plugin:event|unlisten":
              return null;
            case commands.hubStatus:
              return { state: "unpaired" };
            case commands.discover:
              calls.push({ cmd, args: null });
              return {
                ok: true,
                body: { search: calls.filter((c) => c.cmd === commands.discover).length },
              };
            case commands.probeIp:
              calls.push({ cmd, args });
              return queued.shift() ?? null;
            case commands.selectHub:
              calls.push({ cmd, args });
              return { ok: true, body: {} };
            case commands.adminExit:
              calls.push({ cmd, args: null });
              return { ok: false, code: "PIN_REJECTED", body: null };
            case commands.adminExitStatus:
              return { ok: true, body: {} };
            case commands.takeRestoreState:
            case commands.reportAppState:
            case commands.heartbeat:
            case commands.theme:
              return null;
            default:
              throw new Error(`unexpected command ${cmd}`);
          }
        },
      };
      w.__shell = {
        emit: (event: string, payload: unknown) => {
          for (const id of listeners[event] ?? []) callbacks.get(id)?.({ event, id, payload });
        },
        listening: (event: string) => (listeners[event] ?? []).length > 0,
      };
    },
    [names.commands, probes] as [typeof names.commands, Answer[]],
  );
  await page.goto("/");
  await expect(page.getByTestId("discovery")).toBeVisible();
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.discoveryDone,
  );
  await expect.poll(() => calls(page, names.commands.discover)).toHaveLength(1);
}

const emit = (page: Page, event: string, payload: unknown) =>
  page.evaluate(
    ([e, p]) =>
      (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
        e as string,
        p,
      ),
    [event, payload] as const,
  );

const calls = (page: Page, cmd: string) =>
  page.evaluate(
    (c) =>
      (window as unknown as { __calls: Array<{ cmd: string; args: unknown }> }).__calls
        .filter((x) => x.cmd === c)
        .map((x) => x.args),
    cmd,
  );

const searchId = async (page: Page) => (await calls(page, names.commands.discover)).length;

const option = (h: DiscoveredHub) =>
  en["player.discovery.hub_option"].replace("{branch}", h.branch_name).replace("{hub}", h.hub_name);

async function noSeriousViolations(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
}

async function noneFound(page: Page) {
  await emit(page, names.events.discoveryDone, { search: await searchId(page), hubs: [] });
  await expect(page.getByLabel(en["player.discovery.ip_label"])).toBeFocused();
}

test("an unpaired kiosk searches and lists the Hubs it finds", async ({ page }) => {
  await openUnpaired(page);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["player.discovery.title"]);
  await expect(page.getByTestId("discovery-busy")).toBeVisible();
  const status = page.getByTestId("discovery-status");
  await expect(status).toHaveAttribute("aria-live", "polite");
  await expect(status).toHaveText(en["player.discovery.searching"]);
  await expect(page.getByTestId("attract")).toHaveCount(0);

  await emit(page, names.events.discoveryFound, { search: await searchId(page), hubs: [desk] });
  await expect(page.getByRole("button", { name: option(desk) })).toBeEnabled();
  await emit(page, names.events.discoveryFound, {
    search: await searchId(page),
    hubs: [desk, old],
  });
  const incompatible = page.getByRole("button", { name: option(old) });
  await expect(incompatible).toBeDisabled();
  await expect(page.getByText(en["player.discovery.incompatible"])).toBeVisible();
  await expect(status).toHaveText(en["player.discovery.announce_found"].replace("{count}", "2"));
  for (const b of await page.getByRole("button").all()) {
    const box = await b.boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(44);
  }
  await noSeriousViolations(page);

  await emit(page, names.events.discoveryDone, { search: await searchId(page), hubs: [desk, old] });
  await expect(page.getByTestId("discovery-busy")).toHaveCount(0);
  await expect(page.getByLabel(en["player.discovery.ip_label"])).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: en["player.discovery.search_again"] }),
  ).toBeVisible();
});

test("picking a found Hub selects it and moves to the pairing step", async ({ page }) => {
  await openUnpaired(page);
  await emit(page, names.events.discoveryFound, { search: await searchId(page), hubs: [desk] });
  await page.getByRole("button", { name: option(desk) }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["player.pairing.title"]);
  await expect(page.getByLabel(en["player.pairing.code_label"])).toBeVisible();
  expect(await calls(page, names.commands.selectHub)).toEqual([
    { addr: desk.addr, fingerprint: desk.fingerprint },
  ]);
  await expect(page.getByTestId("discovery-status")).toHaveText(en["player.pairing.title"]);
  await noSeriousViolations(page);

  await page.getByRole("button", { name: en["player.pairing.change_hub"] }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["player.discovery.title"]);
  await expect.poll(() => calls(page, names.commands.discover)).toHaveLength(2);
});

test("with nothing found the IP field appears, and a failed check keeps it focused", async ({
  page,
}) => {
  await openUnpaired(page, [
    { ok: false, code: "NO_HUB", body: null },
    { ok: false, code: "HUB_INCOMPATIBLE", body: null },
  ]);
  await noneFound(page);
  await expect(page.getByText(en["player.discovery.ip_hint"])).toBeVisible();
  await expect(page.getByTestId("discovery-status")).toHaveText(en["player.discovery.none"]);
  await noSeriousViolations(page);

  const field = page.getByLabel(en["player.discovery.ip_label"]);
  await field.fill("192.168.1.99");
  await page.getByRole("button", { name: en["player.discovery.check"] }).click();
  await expect(page.getByTestId("ip-error")).toContainText(en["player.discovery.no_hub"]);
  await expect(field).toHaveValue("192.168.1.99");
  await expect(field).toBeFocused();
  await expect(field).toHaveAttribute("aria-invalid", "true");
  await noSeriousViolations(page);

  await field.press("Enter");
  await expect(page.getByTestId("ip-error")).toContainText(en["player.discovery.hub_incompatible"]);
  await expect(field).toBeFocused();
  expect(await calls(page, names.commands.probeIp)).toEqual([
    { ip: "192.168.1.99" },
    { ip: "192.168.1.99" },
  ]);
  expect(await calls(page, names.commands.selectHub)).toEqual([]);
});

test("a valid typed IP is selected and the pairing step follows", async ({ page }) => {
  const typed = { ...desk, addr: "10.0.0.5:7400" };
  await openUnpaired(page, [{ ok: true, body: { hub: typed } }]);
  await noneFound(page);
  await page.getByLabel(en["player.discovery.ip_label"]).fill("10.0.0.5");
  await page.getByRole("button", { name: en["player.discovery.check"] }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["player.pairing.title"]);
  expect(await calls(page, names.commands.selectHub)).toEqual([
    { addr: typed.addr, fingerprint: typed.fingerprint },
  ]);
});

test("Search again starts a new search", async ({ page }) => {
  await openUnpaired(page);
  await noneFound(page);
  await page.getByRole("button", { name: en["player.discovery.search_again"] }).click();
  await expect(page.getByTestId("discovery-busy")).toBeVisible();
  await expect(page.getByLabel(en["player.discovery.ip_label"])).toHaveCount(0);
  await expect.poll(() => calls(page, names.commands.discover)).toHaveLength(2);
});

test("the admin-exit combination still opens the PIN panel on this screen", async ({ page }) => {
  await openUnpaired(page);
  await page.keyboard.press("Control+Alt+Shift+KeyA");
  const panel = page.getByTestId("admin-exit-panel");
  await expect(panel).toBeVisible();
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(panel).toContainText(en["ui.pin.rejected"]);
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  await expect(page.getByTestId("discovery")).toBeVisible();
});

test("the busy indicator does not spin with reduced motion", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openUnpaired(page);
  const spin = await page.locator(".spinner").evaluate((el) => getComputedStyle(el).animationName);
  expect(spin).toBe("none");
});

test("a stale discovery.done from an earlier search is ignored", async ({ page }) => {
  await openUnpaired(page);
  await noneFound(page);
  await page.getByRole("button", { name: en["player.discovery.search_again"] }).click();
  await expect.poll(() => calls(page, names.commands.discover)).toHaveLength(2);
  await expect(page.getByTestId("discovery-busy")).toBeVisible();
  await emit(page, names.events.discoveryDone, { search: 1, hubs: [] });
  await emit(page, names.events.discoveryFound, { search: 1, hubs: [desk] });
  await page.waitForTimeout(200);
  await expect(page.getByTestId("discovery-busy")).toBeVisible();
  await expect(page.getByRole("button", { name: option(desk) })).toHaveCount(0);
  await emit(page, names.events.discoveryDone, { search: 2, hubs: [] });
  await expect(page.getByLabel(en["player.discovery.ip_label"])).toBeFocused();
});

test("without a discovery.done the IP form appears after the window and a grace", async ({
  page,
}) => {
  await page.clock.install();
  await openUnpaired(page);
  await expect(page.getByTestId("discovery-busy")).toBeVisible();
  await page.clock.fastForward(11_000);
  await expect(page.getByTestId("discovery-busy")).toBeVisible();
  await page.clock.fastForward(1_500);
  await expect(page.getByTestId("discovery-busy")).toHaveCount(0);
  await expect(page.getByLabel(en["player.discovery.ip_label"])).toBeVisible();
});

test("a remote address is selected like an IP and pairs without a certificate line", async ({
  page,
}) => {
  const remote = {
    ...desk,
    addr: "01jbranch.tunnel.arena360.cloud:443",
    hub_name: "Remote",
    fingerprint: "",
    web_port: 443,
  };
  await openUnpaired(page, [{ ok: true, body: { hub: remote } }]);
  await noneFound(page);
  await page.getByLabel(en["player.discovery.ip_label"]).fill("01JBRANCH.tunnel.arena360.cloud");
  await page.getByRole("button", { name: en["player.discovery.check"] }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["player.pairing.title"]);
  await expect(page.getByTestId("selected-hub")).toContainText("Remote");
  await expect(page.getByTestId("selected-fingerprint")).toHaveCount(0);
  expect(await calls(page, names.commands.selectHub)).toEqual([
    { addr: remote.addr, fingerprint: "" },
  ]);
});
