import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

async function openWithShell(page: Page, screen: unknown = null) {
  await page.addInitScript(
    ([commands, initial]: [typeof names.commands, unknown]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event: string, id: number) => {
          listeners[event] = (listeners[event] ?? []).filter((x) => x !== id);
        },
      };
      w.__dismissed = 0;
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
            case commands.pcScreen:
              return initial;
            case commands.pcDismiss:
              (w.__dismissed as number)++;
              return null;
            case commands.hubStatus:
            case commands.takeRestoreState:
            case commands.reportAppState:
            case commands.heartbeat:
            case commands.adminExitStatus:
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
    [names.commands, screen] as [typeof names.commands, unknown],
  );
  await page.goto("/");
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.pcScreen,
  );
}

const emit = (page: Page, payload: unknown) =>
  page.evaluate(
    (p) =>
      (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
        "pc-screen",
        p,
      ),
    payload,
  );

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

test("the lock covers the screen with Locked by staff and unlock removes it", async ({ page }) => {
  await openWithShell(page);
  await expect(page.getByTestId("locked")).toBeVisible();
  await expect(page.getByTestId("pc-locked")).toHaveCount(0);
  await emit(page, { locked: true, message: null });
  const cover = page.getByTestId("pc-locked");
  await expect(cover).toBeVisible();
  await expect(cover.getByRole("heading", { name: en["player.pc.locked"] })).toBeVisible();
  expect(await page.locator(".app").getAttribute("inert")).not.toBeNull();
  expect(await serious(page)).toEqual([]);
  await emit(page, { locked: false, message: null });
  await expect(cover).toHaveCount(0);
  expect(await page.locator(".app").getAttribute("inert")).toBeNull();
});

test("a message shows its text until the player dismisses it", async ({ page }) => {
  await openWithShell(page);
  await emit(page, { locked: false, message: "Closing in 10 minutes" });
  const banner = page.getByTestId("pc-message");
  await expect(banner).toContainText("Closing in 10 minutes");
  await expect(banner).toContainText(en["player.pc.message_title"]);
  expect(await serious(page)).toEqual([]);
  await emit(page, { locked: false, message: "Closing in 10 minutes" });
  await expect(banner).toHaveCount(1);
  await banner.getByRole("button", { name: en["common.dismiss"] }).click();
  await expect(banner).toHaveCount(0);
  expect(
    await page.evaluate(() => (window as unknown as { __dismissed: number }).__dismissed),
  ).toBe(1);
});

test("a lock that was already on when the page opened covers it at once", async ({ page }) => {
  await openWithShell(page, { locked: true, message: "Back soon" });
  await expect(page.getByTestId("pc-locked")).toBeVisible();
  await expect(page.getByTestId("pc-message")).toContainText("Back soon");
});
