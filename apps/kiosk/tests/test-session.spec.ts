import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

async function openWithShell(page: Page, running: number | null = null) {
  await page.addInitScript(
    ([commands, remaining]: [typeof names.commands, number | null]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
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
            case commands.testSession:
              return remaining;
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
    [names.commands, running] as [typeof names.commands, number | null],
  );
  await page.goto("/");
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.testSessionEnded,
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

test("a test session shows its title and counts down to the locked screen", async ({ page }) => {
  await openWithShell(page);
  await expect(page.getByTestId("locked")).toBeVisible();
  await emit(page, names.events.testSession, { remaining_ms: 2500 });
  await expect(page.getByRole("heading", { name: en["player.test_session.title"] })).toBeVisible();
  const clock = page.getByTestId("test-session-clock");
  await expect(clock).toHaveText("0:03");
  expect(await clock.evaluate((el) => getComputedStyle(el).fontVariantNumeric)).toContain(
    "tabular-nums",
  );
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
  await expect(clock).toHaveText("0:00", { timeout: 5000 });
  await emit(page, names.events.testSessionEnded, null);
  await expect(page.getByTestId("locked")).toBeVisible();
  await expect(page.getByTestId("test-session")).toHaveCount(0);
});

test("a test already running when the page opens shows two minutes", async ({ page }) => {
  await openWithShell(page, 120_000);
  await expect(page.getByTestId("test-session-clock")).toHaveText(/^(2:00|1:59)$/);
});
