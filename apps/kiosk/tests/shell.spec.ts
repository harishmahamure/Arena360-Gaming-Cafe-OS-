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
            case commands.attract:
              return null;
            case commands.staffMessages:
              return [];
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

const shown = (session: Record<string, unknown> | null, over: Record<string, unknown> = {}) => ({
  locked: false,
  reason: null,
  session: session && { device_name: "PC 7", elapsed_ms: 0, ...session },
  ...over,
});

test("a prepaid session opens the shell with the profiled player and a counting-down chip", async ({
  page,
}) => {
  await openWithShell(page);
  await emit(page, shown({ username: "asha", remaining_ms: 3_725_000 }));
  await expect(page.getByTestId("shell")).toBeVisible();
  await expect(page.getByTestId("account-name")).toHaveText("asha");
  await expect(page.getByTestId("account-chip")).toContainText("A");
  await expect(page.getByTestId("session-device")).toHaveText("PC 7");
  await expect(page.getByTestId("session-clock")).toHaveText(/^1:02:0[0-5]$/);
  await expect(page.getByTestId("time-chip")).not.toHaveClass(/low/);
  await emit(page, shown({ username: "asha", remaining_ms: 3_700_000 }));
  await expect(page.getByTestId("session-clock")).toHaveText(/^1:01:[3-4]\d$/);
  expect(await serious(page)).toEqual([]);
});

test("the top bar sits 32px down, 64px tall, with logo, nav, chip and account in order", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openWithShell(page);
  await emit(page, shown({ guest_no: 3 }));
  const bar = page.locator(".bar");
  const box = (await bar.boundingBox())!;
  expect(Math.round(box.y)).toBe(32);
  expect(Math.round(box.height)).toBe(64);
  const x = async (id: string) => (await page.getByTestId(id).boundingBox())!.x;
  expect(await x("shell-logo")).toBeLessThan(await x("pill-nav"));
  expect(await x("pill-nav")).toBeLessThan(await x("time-chip"));
  expect(await x("time-chip")).toBeLessThan(await x("account-chip"));
});

test("a postpaid guest is Guest <n> and the chip counts up", async ({ page }) => {
  await openWithShell(page);
  await emit(page, shown({ guest_no: 3, elapsed_ms: 65_000 }));
  await expect(page.getByTestId("account-name")).toHaveText("Guest 3");
  await expect(page.getByTestId("session-clock")).toHaveText(/^0:01:0[5-9]$/);
  await expect(page.getByTestId("time-chip")).not.toHaveClass(/low/);
});

test("at 10 minutes or less the chip gets a warning border and a warning glyph", async ({
  page,
}) => {
  await openWithShell(page);
  await emit(page, shown({ username: "asha", remaining_ms: 601_000 }));
  await expect(page.getByTestId("time-chip")).not.toHaveClass(/low/);
  await expect(page.getByTestId("time-chip-dot")).toHaveText("");
  await emit(page, shown({ username: "asha", remaining_ms: 600_000 }));
  const chip = page.getByTestId("time-chip");
  await expect(chip).toHaveClass(/low/);
  await expect(page.getByTestId("time-chip-dot")).toHaveText("⚠");
  expect(await chip.evaluate((el) => getComputedStyle(el).borderTopWidth)).toBe("2px");
  expect(await serious(page)).toEqual([]);
});

test("the pill nav starts on Home, arrows move focus, Enter selects and the session end resets it", async ({
  page,
}) => {
  await openWithShell(page);
  await emit(page, shown({ username: "asha", remaining_ms: 3_000_000 }));
  const home = page.getByTestId("tab-home");
  const games = page.getByTestId("tab-games");
  const apps = page.getByTestId("tab-apps");
  await expect(home).toHaveAttribute("aria-current", "page");
  await expect(games).not.toHaveAttribute("aria-current", "page");
  await home.focus();
  await page.keyboard.press("ArrowRight");
  await expect(games).toBeFocused();
  await expect(home).toHaveAttribute("aria-current", "page");
  await page.keyboard.press("Enter");
  await expect(games).toHaveAttribute("aria-current", "page");
  await expect(home).not.toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("tab-empty-games")).toHaveText(en["player.shell.empty"]);
  await page.keyboard.press("ArrowRight");
  await expect(apps).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect(home).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(apps).toBeFocused();
  await apps.click();
  await expect(apps).toHaveAttribute("aria-current", "page");
  // The selection persists through ticks of the same session...
  await emit(page, shown({ username: "asha", remaining_ms: 2_990_000 }));
  await expect(apps).toHaveAttribute("aria-current", "page");
  // ...and resets when the session ends.
  await emit(page, shown(null));
  await emit(page, shown({ username: "ben", remaining_ms: 3_000_000 }));
  await expect(page.getByTestId("tab-home")).toHaveAttribute("aria-current", "page");
});

test("a lock reason covers the shell with a solid screen, the logo slot and its line", async ({
  page,
}) => {
  await openWithShell(page);
  await emit(page, shown({ username: "asha", remaining_ms: 3_000_000 }));
  await emit(
    page,
    shown(
      { username: "asha", remaining_ms: 3_000_000 },
      { locked: true, reason: "ended_by_staff" },
    ),
  );
  const cover = page.getByTestId("pc-locked");
  await expect(cover).toBeVisible();
  await expect(cover.getByTestId("lock-logo")).toBeVisible();
  await expect(
    cover.getByRole("heading", { name: en["player.lock.ended_by_staff"] }),
  ).toBeVisible();
  expect(await cover.evaluate((el) => getComputedStyle(el).backgroundColor)).not.toMatch(
    /rgba\(.*, 0\)/,
  );
  await emit(page, shown({ username: "asha", remaining_ms: 2_000_000 }));
  await expect(cover).toHaveCount(0);
  await expect(page.getByTestId("shell")).toBeVisible();
});

test("a session already running when the page opened shows the shell at once", async ({ page }) => {
  await openWithShell(page, shown({ username: "asha", remaining_ms: 120_000 }));
  await expect(page.getByTestId("shell")).toBeVisible();
  await expect(page.getByTestId("time-chip")).toHaveClass(/low/);
});
