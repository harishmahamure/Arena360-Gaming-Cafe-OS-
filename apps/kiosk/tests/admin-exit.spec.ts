import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

// The hidden admin exit (Story 1.20): Ctrl+Alt+Shift+A opens a manager PIN panel, the PIN goes
// to the shell's `admin_exit` command (stubbed here with the service's answers), and a started
// exit turns the window into the countdown badge until `admin-exit-ended`.

type Answer = { ok: boolean; code?: string; body: unknown };

async function openWithShell(page: Page, answers: Answer[]) {
  await page.addInitScript(
    ([commands, queued]: [typeof names.commands, Answer[]]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      const pins: string[] = [];
      w.__pins = pins;
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
            case commands.adminExit:
              pins.push(args.pin as string);
              return queued.shift() ?? null;
            case commands.adminExitStatus:
              return { ok: true, body: {} };
            case commands.hubStatus:
            case commands.takeRestoreState:
            case commands.reportAppState:
            case commands.heartbeat:
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
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.adminExitEnded,
  );
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

const combo = (page: Page) => page.keyboard.press("Control+Alt+Shift+KeyA");

test("nothing on screen hints at the admin exit until the combination is pressed", async ({
  page,
}) => {
  await openWithShell(page, []);
  await expect(page.getByTestId("admin-exit-panel")).toHaveCount(0);
  await expect(page.getByText(en["player.admin_exit.title"])).toHaveCount(0);
  await combo(page);
  const panel = page.getByTestId("admin-exit-panel");
  await expect(panel).toBeVisible();
  await expect(panel).toContainText(en["player.admin_exit.title"]);
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
  // Esc closes it.
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
});

test("a wrong PIN clears the cells with the generic message; five pause with a countdown", async ({
  page,
}) => {
  await openWithShell(page, [
    { ok: false, code: "PIN_REJECTED", body: null },
    { ok: false, code: "AUTH_LOCKED", body: { retry_after_s: 60 } },
  ]);
  await combo(page);
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  const panel = page.getByTestId("admin-exit-panel");
  await expect(panel).toContainText(en["ui.pin.rejected"]);
  await expect(panel.locator("input")).toHaveValue("");
  await page.keyboard.type("9999");
  await page.keyboard.press("Enter");
  await expect(panel).toContainText(en["ui.pin.locked"].replace("{seconds}", "60"));
  expect(await page.evaluate(() => (window as unknown as { __pins: string[] }).__pins)).toEqual([
    "9999",
    "9999",
  ]);
});

test("a granted exit shows only the countdown badge until it ends", async ({ page }) => {
  const until = Date.now() + 15 * 60 * 1000;
  await openWithShell(page, [{ ok: true, body: { until } }]);
  await combo(page);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  const badge = page.getByTestId("admin-exit-badge");
  await expect(badge).toBeVisible();
  await expect(badge).toContainText(/Admin exit: 1[45]:\d\d left/);
  await expect(page.getByTestId("admin-exit-panel")).toHaveCount(0);
  await expect(page.getByTestId("attract")).toBeHidden();
  // The combination does nothing while an exit runs.
  await combo(page);
  await expect(page.getByTestId("admin-exit-panel")).toHaveCount(0);
  await emit(page, names.events.adminExitEnded, null);
  await expect(badge).toHaveCount(0);
  await expect(page.getByTestId("attract")).toBeVisible();
});

test("a remote unlock pushed by the service shows the badge", async ({ page }) => {
  await openWithShell(page, []);
  await emit(page, names.events.adminExit, { until: Date.now() + 15 * 60 * 1000 });
  await expect(page.getByTestId("admin-exit-badge")).toContainText(
    en["player.admin_exit.badge"].split(":")[0],
  );
});
