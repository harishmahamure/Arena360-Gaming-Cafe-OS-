import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

// The toast the Kiosk service raises after it closed a blocked app (Story 1.18). The Tauri
// shell is stubbed in the page, and the test emits its `toast` event exactly as
// `@tauri-apps/api/event` delivers it.

async function openWithShell(page: Page) {
  await page.addInitScript((commands: typeof names.commands) => {
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
  }, names.commands);
  await page.goto("/");
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.toast,
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

test("a blocked app shows a polite toast that stays until dismissed and never takes focus", async ({
  page,
}) => {
  await openWithShell(page);
  const region = page.getByTestId("toasts");
  await expect(region).toHaveAttribute("aria-live", "polite");
  await expect(page.getByTestId("toast")).toHaveCount(0);
  const focusedBefore = await page.evaluate(() => document.activeElement?.tagName);

  await emit(page, names.events.toast, "player.app.blocked");
  const toast = page.getByTestId("toast");
  await expect(toast).toHaveCount(1);
  await expect(toast).toContainText(en["player.app.blocked"]);
  expect(await page.evaluate(() => document.activeElement?.tagName)).toBe(focusedBefore);

  // Persistent: still there later.
  await page.waitForTimeout(1500);
  await expect(toast).toHaveCount(1);

  // Repeats collapse into the same toast.
  await emit(page, names.events.toast, "player.app.blocked");
  await emit(page, names.events.toast, "player.app.blocked");
  await expect(page.getByTestId("toast")).toHaveCount(1);

  // Unknown keys are ignored.
  await emit(page, names.events.toast, "player.nope");
  await expect(page.getByTestId("toast")).toHaveCount(1);

  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);

  await page.getByRole("button", { name: en["common.dismiss"] }).click();
  await expect(page.getByTestId("toast")).toHaveCount(0);

  // A later kill shows it again.
  await emit(page, names.events.toast, "player.app.blocked");
  await expect(page.getByTestId("toast")).toHaveCount(1);
});

test("the toast region lets clicks through outside the toast", async ({ page }) => {
  await openWithShell(page);
  await emit(page, names.events.toast, "player.app.blocked");
  await expect(page.getByTestId("toast")).toHaveCount(1);
  const hitsRegion = await page.evaluate(() => {
    const region = document.querySelector("[data-testid=toasts]")!.getBoundingClientRect();
    const el = document.elementFromPoint(region.x + 4, region.y + region.height / 2);
    return el?.closest("[data-testid=toasts]") !== null;
  });
  expect(hitsRegion).toBe(false);
});
