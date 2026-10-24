import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { KioskHubStatus } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

// The Kiosk UI's link status and state restore. The Tauri shell is stubbed in the page: the
// stub records `invoke` calls and lets the test emit the shell's `hub-status` and
// `restore-state` events exactly as `@tauri-apps/api/event` delivers them.

type Stub = {
  hub: KioskHubStatus | null;
  restore: unknown;
};

async function openWithShell(page: Page, stub: Stub, path = "/") {
  await page.addInitScript(
    ([initial, commands]: [Stub, typeof names.commands]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      const reported: unknown[] = [];
      let heartbeats = 0;
      let next = 1;
      let restore = initial.restore;
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
              return initial.hub;
            case commands.takeRestoreState: {
              const r = restore;
              restore = null;
              return r;
            }
            case commands.reportAppState:
              reported.push(args.state);
              return null;
            case commands.heartbeat:
              heartbeats++;
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
        reported,
        heartbeats: () => heartbeats,
        listening: (event: string) => (listeners[event] ?? []).length > 0,
      };
    },
    [stub, names.commands] as [Stub, typeof names.commands],
  );
  await page.goto(path);
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.hubStatus,
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

const reported = (page: Page) =>
  page.evaluate(() => (window as unknown as { __shell: { reported: unknown[] } }).__shell.reported);

test("the Reconnecting line appears and clears with the hub-status event", async ({ page }) => {
  await openWithShell(page, { hub: { state: "connected" }, restore: null });
  const status = page.getByRole("status");
  await expect(status).toHaveAttribute("aria-live", "polite");
  await expect(status).toHaveText("");

  await emit(page, names.events.hubStatus, { state: "reconnecting" });
  await expect(status).toHaveText(en["player.hub.reconnecting"]);
  // Non-blocking: the screen under it still takes clicks.
  const box = await status.boundingBox();
  expect(box).not.toBeNull();
  const hit = await page.evaluate(
    ([x, y]) => document.elementFromPoint(x, y)?.closest("[role=status]") === null,
    [box!.x + box!.width / 2, box!.y + box!.height / 2] as const,
  );
  expect(hit).toBe(true);
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);

  await emit(page, names.events.hubStatus, { state: "connected" });
  await expect(status).toHaveText("");
  await emit(page, names.events.hubStatus, { state: "token_rejected" });
  await expect(status).toHaveText(en["player.hub.token_rejected"]);
  // Junk is ignored.
  await emit(page, names.events.hubStatus, { state: "bogus" });
  await expect(status).toHaveText(en["player.hub.token_rejected"]);
});

test("the webview sends a heartbeat every second", async ({ page }) => {
  await openWithShell(page, { hub: null, restore: null });
  const beats = () =>
    page.evaluate(() =>
      (window as unknown as { __shell: { heartbeats: () => number } }).__shell.heartbeats(),
    );
  const first = await beats();
  await page.waitForTimeout(2200);
  expect((await beats()) - first).toBeGreaterThanOrEqual(2);
});

test("a state the shell already knows is shown at once", async ({ page }) => {
  await openWithShell(page, { hub: { state: "reconnecting" }, restore: null });
  await expect(page.getByRole("status")).toHaveText(en["player.hub.reconnecting"]);
});

test("the app reports its route and restores it after a restart", async ({ page }) => {
  await openWithShell(page, { hub: null, restore: null });
  await expect.poll(() => reported(page)).toContainEqual({ route: "/" });

  // A restore-state event moves the app back to where the player was.
  await emit(page, names.events.restoreState, { route: "/?screen=games" });
  await expect(page).toHaveURL(/\/\?screen=games$/);
  await expect.poll(() => reported(page)).toContainEqual({ route: "/?screen=games" });
  // Never to another origin.
  await emit(page, names.events.restoreState, { route: "//evil.example/" });
  await page.waitForTimeout(200);
  await expect(page).toHaveURL(/127\.0\.0\.1:\d+\/\?screen=games$/);
});

test("a restore that arrived before the page listened is applied on load", async ({ page }) => {
  await openWithShell(page, { hub: null, restore: { route: "/?screen=shop" } });
  await expect(page).toHaveURL(/\/\?screen=shop$/);
});

test("the app fills the whole window", async ({ page }) => {
  await openWithShell(page, { hub: null, restore: null });
  const size = await page.locator(".kiosk").evaluate((el) => {
    const r = el.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height];
  });
  expect(size).toEqual([0, 0, 1920, 1080]);
});
