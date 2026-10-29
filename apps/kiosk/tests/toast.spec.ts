import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

// The toast the Kiosk service raises after it closed a blocked app (Story 1.18), shown on the
// overlay HUD as a persistent warning `hud-toast` (Story 6.14). The Tauri
// shell is stubbed in the page, and the test emits its `toast` event exactly as
// `@tauri-apps/api/event` delivers it.

async function openWithShell(page: Page, path = "/hud") {
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
          case commands.pcScreen:
          case commands.attract:
          case commands.theme:
          case commands.hudLayout:
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
  await page.goto(path);
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

test("the toast is the 640px solid hud-toast with an amber border and no blur", async ({
  page,
}) => {
  await openWithShell(page);
  await emit(page, names.events.toast, "player.app.blocked");
  const toast = page.getByTestId("toast");
  await expect(toast).toHaveCount(1);
  expect(Math.round((await toast.boundingBox())!.width)).toBe(640);
  const style = (prop: string) =>
    toast.evaluate((el, p) => getComputedStyle(el).getPropertyValue(p), prop);
  expect(await style("background-color")).toBe("rgb(30, 22, 6)");
  expect(await style("border-top-color")).toBe("rgb(255, 190, 61)");
  expect(await style("border-top-width")).toBe("2px");
  expect(await style("backdrop-filter")).toBe("none");
});

test("Call staff shows an info toast that goes by itself, and so does the reconnecting line", async ({
  page,
}) => {
  await openWithShell(page);
  await emit(page, names.events.toast, "player.call_staff.sent");
  const toast = page.getByTestId("toast");
  await expect(toast).toHaveCount(1);
  await expect(toast).toHaveAttribute("data-kind", "info");
  await expect(toast).toContainText(en["player.call_staff.sent"]);
  // The same text again replaces the one showing.
  await emit(page, names.events.toast, "player.call_staff.sent");
  await expect(toast).toHaveCount(1);
  await emit(page, names.events.toast, "player.hub.reconnecting_retry");
  await expect(toast).toHaveCount(2);
  await expect(toast.last()).toContainText(en["player.hub.reconnecting_retry"]);
  await expect(toast.last()).toHaveAttribute("data-kind", "info");
});

test("a fresh staff message is an info toast with its text that goes by itself, and a replay is not raised", async ({
  page,
}) => {
  await openWithShell(page);
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.staffMessage,
  );
  const toast = page.getByTestId("toast");
  const message = (id: number, text: string, fresh: boolean) => ({
    id,
    text,
    at: Date.UTC(2026, 9, 29, 8, 30),
    read: false,
    fresh,
  });
  await emit(page, names.events.staffMessage, message(1, "Closing at 11", false));
  await expect(toast).toHaveCount(0);
  await emit(page, names.events.staffMessage, message(2, "Closing at 11", true));
  await expect(toast).toHaveCount(1);
  await expect(toast).toHaveAttribute("data-kind", "info");
  await expect(toast).toContainText("Closing at 11");
  expect(Math.round((await toast.boundingBox())!.width)).toBe(640);
  expect(await toast.evaluate((el) => getComputedStyle(el).backdropFilter)).toBe("none");
  await emit(page, names.events.staffMessage, message(3, "Your order is ready", true));
  await expect(toast).toHaveCount(2);
  await expect(toast.last()).toContainText("Your order is ready");
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
  await toast.first().getByRole("button", { name: en["common.dismiss"] }).click();
  await expect(toast).toHaveCount(1);
});
