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
  web_port: 7401,
  compatible: true,
};

const paired: Answer = {
  ok: true,
  body: { device_id: "01JDEVICEAAAAAAAAAAAAAAAAA", branch_name: "Koramangala" },
};

async function openPairing(page: Page, answers: Answer[]) {
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
              return { ok: true, body: { search: 1 } };
            case commands.selectHub:
              return { ok: true, body: {} };
            case commands.pairInfo:
              return { ok: true, body: { pc_name: "FRONT-PC-3", hub: null } };
            case commands.pair: {
              calls.push({ cmd, args });
              const next = queued.shift() ?? null;
              if (next && next.code === "HOLD") {
                return new Promise((resolve) => {
                  (w as Record<string, unknown>).__release = () => resolve(next.body);
                });
              }
              return next;
            }
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
    [names.commands, answers] as [typeof names.commands, Answer[]],
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
  await emit(page, names.events.discoveryFound, { search: 1, hubs: [desk] });
  await page.getByRole("button", { name: option(desk) }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["player.pairing.title"]);
  await expect(page.getByLabel(en["player.pairing.pc_name"])).toHaveValue("FRONT-PC-3");
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

const pairCalls = (page: Page) =>
  page.evaluate(
    (c) =>
      (window as unknown as { __calls: Array<{ cmd: string; args: unknown }> }).__calls
        .filter((x) => x.cmd === c)
        .map((x) => x.args),
    names.commands.pair,
  );

const option = (h: DiscoveredHub) =>
  en["player.discovery.hub_option"].replace("{branch}", h.branch_name).replace("{hub}", h.hub_name);

async function noSeriousViolations(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
}

const codeInput = (page: Page) => page.getByLabel(en["player.pairing.code_label"]);

test("a wrong code clears the cells with one plain line, and the right one pairs", async ({
  page,
}) => {
  await openPairing(page, [{ ok: false, code: "PAIR_CODE_INVALID", body: null }, paired]);
  await noSeriousViolations(page);
  await codeInput(page).fill("123456");
  await expect(page.getByText(en["player.pairing.code_invalid"])).toBeVisible();
  await expect(codeInput(page)).toHaveValue("");
  await expect(codeInput(page)).toBeFocused();
  await expect(codeInput(page)).toHaveAttribute("aria-invalid", "true");
  await noSeriousViolations(page);

  await page.getByLabel(en["player.pairing.pc_name"]).fill("Window seat 4");
  await codeInput(page).fill("654321");
  await expect(page.getByTestId("paired-heading")).toHaveText(
    en["player.pairing.paired"].replace("{branch}", "Koramangala"),
  );
  expect(await pairCalls(page)).toEqual([
    { code: "123456", pcName: "FRONT-PC-3" },
    { code: "654321", pcName: "Window seat 4" },
  ]);
  await expect(page.getByTestId("discovery-status")).toHaveText(
    en["player.pairing.paired"].replace("{branch}", "Koramangala"),
  );
  await noSeriousViolations(page);

  await emit(page, names.events.hubStatus, { state: "reconnecting" });
  await expect(page.getByTestId("paired-heading")).toBeVisible();
  await emit(page, names.events.hubStatus, { state: "connected" });
  await expect(page.getByTestId("discovery")).toHaveCount(0);
  await expect(page.getByTestId("locked")).toBeVisible();
});

test("a full branch and too many tries each get one plain line", async ({ page }) => {
  await openPairing(page, [
    { ok: false, code: "DEVICE_LIMIT", body: null },
    { ok: false, code: "RATE_LIMITED", body: null },
  ]);
  await codeInput(page).fill("123456");
  await expect(page.getByText(en["player.pairing.device_limit"])).toBeVisible();
  await expect(codeInput(page)).toHaveValue("");
  await codeInput(page).fill("123456");
  await expect(page.getByText(en["player.pairing.rate_limited"])).toBeVisible();
  await noSeriousViolations(page);
});

test("an empty PC name is refused before anything is sent", async ({ page }) => {
  await openPairing(page, [paired]);
  const name = page.getByLabel(en["player.pairing.pc_name"]);
  await name.fill("   ");
  await codeInput(page).fill("111111");
  await expect(page.getByTestId("name-error")).toContainText(en["player.pairing.pc_name_invalid"]);
  await expect(name).toBeFocused();
  expect(await pairCalls(page)).toEqual([]);
  await name.fill("PC-9");
  await page.getByRole("button", { name: en["player.pairing.submit"] }).click();
  await expect(page.getByTestId("paired-heading")).toBeVisible();
  expect(await pairCalls(page)).toEqual([{ code: "111111", pcName: "PC-9" }]);
});

test("a manager PIN pairs, a wrong one clears, and a pause disables only the PIN", async ({
  page,
}) => {
  await openPairing(page, [
    { ok: false, code: "PIN_REJECTED", body: null },
    { ok: false, code: "PAIR_PIN_PAUSED", body: { retry_after_s: 60 } },
    paired,
  ]);
  await page.getByRole("button", { name: en["player.pairing.use_pin"] }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["player.pairing.pin_title"]);
  const pin = page.getByTestId("pair-pin");
  await expect(pin).toContainText(en["player.pairing.pin_prompt"]);
  await noSeriousViolations(page);
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(pin).toContainText(en["ui.pin.rejected"]);
  await page.keyboard.type("8888");
  await page.keyboard.press("Enter");

  await expect(page.getByTestId("pair-pin")).toHaveCount(0);
  const toggle = page.getByTestId("use-pin");
  await expect(toggle).toBeDisabled();
  await expect(page.getByTestId("pin-paused")).toContainText(/\b(60|59|58) s\b/);
  await noSeriousViolations(page);

  await codeInput(page).fill("246802");
  await expect(page.getByTestId("paired-heading")).toBeVisible();
  expect(await pairCalls(page)).toEqual([
    { pin: "9999", pcName: "FRONT-PC-3" },
    { pin: "8888", pcName: "FRONT-PC-3" },
    { code: "246802", pcName: "FRONT-PC-3" },
  ]);
});

test("the admin-exit combination still works on the pairing step", async ({ page }) => {
  await openPairing(page, []);
  await page.keyboard.press("Control+Alt+Shift+KeyA");
  const panel = page.getByTestId("admin-exit-panel");
  await expect(panel).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  await expect(page.getByTestId("pair-form")).toBeVisible();
});

test("the pairing step shows the chosen Hub's short fingerprint above the code", async ({
  page,
}) => {
  await openPairing(page, []);
  await expect(page.getByTestId("selected-hub")).toContainText("Koramangala");
  await expect(page.getByTestId("selected-hub")).toContainText("front-desk");
  await expect(page.getByTestId("selected-fingerprint")).toHaveText(
    en["player.pairing.fingerprint"].replace("{fingerprint}", "abab abab abab abab"),
  );
  const fp = await page.getByTestId("selected-fingerprint").boundingBox();
  const field = await codeInput(page).boundingBox();
  expect(fp!.y).toBeLessThan(field!.y);
});

test("Choose a different Hub is locked while a pairing is in flight", async ({ page }) => {
  await openPairing(page, [{ ok: false, code: "HOLD", body: paired }]);
  await codeInput(page).fill("123456");
  const change = page.getByTestId("change-hub");
  await expect(change).toBeDisabled();
  await page.evaluate(() => (window as unknown as { __release: () => void }).__release());
  await expect(page.getByTestId("paired-heading")).toBeVisible();
});

test("specific Hub refusals get their own line", async ({ page }) => {
  await openPairing(page, [
    { ok: false, code: "NOT_PRIMARY", body: null },
    { ok: false, code: "HUB_SLOW", body: null },
    { ok: false, code: "VALIDATION_FAILED", body: null },
    { ok: false, code: "INTERNAL", body: null },
  ]);
  for (const key of [
    "player.pairing.not_primary",
    "player.pairing.hub_slow",
    "player.pairing.refused_name",
    "player.pairing.hub_error",
  ] as const) {
    await codeInput(page).fill("123456");
    await expect(page.getByTestId("pair-code")).toContainText(en[key]);
  }
});

test("a pairing that never connects times out with Retry and Pair again", async ({ page }) => {
  await page.clock.install();
  await openPairing(page, [paired]);
  await codeInput(page).fill("654321");
  await expect(page.getByTestId("paired-heading")).toBeVisible();
  await page.clock.runFor(29_000);
  await expect(page.getByTestId("connect-failed")).toHaveCount(0);
  await page.clock.runFor(1_500);
  await expect(page.getByTestId("connect-failed")).toContainText(
    en["player.pairing.connect_failed"],
  );
  await page.getByRole("button", { name: en["player.pairing.retry"] }).click();
  await expect(page.getByTestId("connect-failed")).toHaveCount(0);
  await page.clock.runFor(31_000);
  await page.getByRole("button", { name: en["player.pairing.pair_again"] }).click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["player.discovery.title"]);
});
