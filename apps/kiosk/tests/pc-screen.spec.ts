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

const shown = (over: Record<string, unknown>) => ({
  locked: false,
  reason: null,
  message: null,
  session: null,
  ...over,
});

test("a started session replaces the idle screen with the device name and a ticking clock", async ({
  page,
}) => {
  await openWithShell(page);
  await expect(page.getByTestId("locked")).toBeVisible();
  await emit(page, shown({ session: { device_name: "PC 7", elapsed_ms: 3_725_000 } }));
  await expect(page.getByTestId("session")).toBeVisible();
  await expect(page.getByTestId("session-device")).toHaveText("PC 7");
  await expect(page.getByTestId("session-clock")).toHaveText(/^1:02:(0[5-9]|1\d)$/);
  await expect(page.getByTestId("locked")).toHaveCount(0);
  expect(await serious(page)).toEqual([]);
  await emit(page, shown({ session: { device_name: "PC 7", elapsed_ms: 5_000 } }));
  await expect(page.getByTestId("session-clock")).toHaveText(/^0:00:0[5-9]$/);
  await emit(page, shown({ locked: true, reason: "times_up" }));
  await expect(page.getByTestId("session")).toHaveCount(0);
  await expect(page.getByTestId("locked")).toBeVisible();
});

test("a session already running when the page opened is shown at once", async ({ page }) => {
  await openWithShell(page, shown({ session: { device_name: "PC 2", elapsed_ms: 61_000 } }));
  await expect(page.getByTestId("session-device")).toHaveText("PC 2");
  await expect(page.getByTestId("session-clock")).toHaveText(/^0:01:0\d$/);
});

test("each lock reason covers the screen with its catalogue line", async ({ page }) => {
  await openWithShell(page);
  const lines: Record<string, string> = {
    pay_at_counter: en["player.lock.pay_at_counter"],
    ended_by_staff: en["player.lock.ended_by_staff"],
    times_up: en["player.lock.times_up"],
    locked_by_staff: en["player.pc.locked"],
    paused: en["player.lock.paused"],
    times_up_see_counter: en["player.lock.times_up_see_counter"],
  };
  for (const [reason, line] of Object.entries(lines)) {
    await emit(page, shown({ locked: true, reason }));
    await expect(page.getByTestId("pc-locked").getByRole("heading", { name: line })).toBeVisible();
  }
  await emit(page, shown({ locked: false }));
  await expect(page.getByTestId("pc-locked")).toHaveCount(0);
});

test("a session in its grace says time is up and counts the grace down", async ({ page }) => {
  await openWithShell(page);
  const session = (grace_in_ms: number) =>
    shown({ session: { device_name: "PC 7", elapsed_ms: 5_000, grace_in_ms, grace_ms: 90_000 } });
  await emit(page, session(60_000));
  await expect(page.getByTestId("session-grace")).toHaveCount(0);
  await emit(page, session(0));
  await expect(page.getByTestId("session-grace")).toContainText(
    /Time's up\. Grace ends in 1:(2|3)\d/,
  );
  expect(await serious(page)).toEqual([]);
});

test("the session screen shows how many items were ordered, and only when there are some", async ({
  page,
}) => {
  await openWithShell(page);
  const session = (ordered_items?: number) =>
    shown({ session: { device_name: "PC 7", elapsed_ms: 5_000, ordered_items } });
  await emit(page, session());
  await expect(page.getByTestId("session")).toBeVisible();
  await expect(page.getByTestId("ordered")).toHaveCount(0);
  await emit(page, session(3));
  await expect(page.getByTestId("ordered")).toHaveText("Ordered: 3 items");
  expect(await serious(page)).toEqual([]);
});
