import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

// The kiosk's high contrast (Story 1.21). The Tauri shell is stubbed in the page: its
// initialization script (the shell sets `data-theme` before any page script runs) is the real
// `theme::init_script` output for `preset` (fixtures a Rust test keeps identical), its `theme` command answers `hc`, and the test emits its `theme` event exactly as
// `@tauri-apps/api/event` delivers it.

type Shell = { preset: "hc" | "dark" | null; hc: boolean | null };

const initScript = (preset: "hc" | "dark") =>
  decodeURIComponent(new URL(`./fixtures/theme-init-${preset}.js`, import.meta.url).pathname);

async function openWithShell(page: Page, shell: Shell) {
  if (shell.preset) await page.addInitScript({ path: initScript(shell.preset) });
  await page.addInitScript(
    ([s, commands]: [Shell, typeof names.commands]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      // The first theme the page ever had (before any app script could change it).
      w.__firstTheme = null;
      new MutationObserver((_, o) => {
        if (document.documentElement) {
          w.__firstTheme = document.documentElement.dataset.theme ?? "";
          o.disconnect();
        }
      }).observe(document, { childList: true });
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
            case commands.theme:
              return s.hc;
            case commands.attract:
              return null;
            case commands.hubStatus:
            case commands.takeRestoreState:
            case commands.reportAppState:
            case commands.heartbeat:
            case commands.adminExitStatus:
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
    [shell, names.commands] as [Shell, typeof names.commands],
  );
  await page.goto("/");
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.theme,
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

const theme = (page: Page) => page.evaluate(() => document.documentElement.dataset.theme);

test("a kiosk the shell started in HC is HC from its first paint", async ({ page }) => {
  await openWithShell(page, { preset: "hc", hc: true });
  expect(
    await page.evaluate(() => (window as unknown as { __firstTheme: string }).__firstTheme),
  ).toBe("hc");
  expect(await theme(page)).toBe("hc");
  // Pure black ground, white ink.
  const kiosk = page.locator(".kiosk");
  expect(await kiosk.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgb(0, 0, 0)");
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).color)).toBe(
    "rgb(255, 255, 255)",
  );
});

test("a manager's change applies live, without a reload", async ({ page }) => {
  await openWithShell(page, { preset: "dark", hc: false });
  expect(await theme(page)).toBe("dark");
  const marker = await page.evaluate(() => ((window as unknown as { __m: number }).__m = 42));
  await emit(page, names.events.theme, { hc: true });
  await expect.poll(() => theme(page), { timeout: 2000 }).toBe("hc");
  await emit(page, names.events.theme, { hc: false });
  await expect.poll(() => theme(page), { timeout: 2000 }).toBe("dark");
  // Junk is ignored.
  await emit(page, names.events.theme, { hc: "yes" });
  await page.waitForTimeout(100);
  expect(await theme(page)).toBe("dark");
  // Same page: nothing restarted.
  expect(await page.evaluate(() => (window as unknown as { __m: number }).__m)).toBe(marker);
});

test("the page takes the shell's current theme when its init script is older", async ({ page }) => {
  // The shell's init script still says dark, but the service has since said HC.
  await openWithShell(page, { preset: "dark", hc: true });
  await expect.poll(() => theme(page)).toBe("hc");
});

test("the HC placeholder and toast pass axe with HC outlines, focus ring and no motion", async ({
  page,
}) => {
  await openWithShell(page, { preset: "hc", hc: true });
  await emit(page, names.events.toast, "player.app.blocked");
  const dismiss = page.getByRole("button", { name: en["common.dismiss"] });
  await expect(dismiss).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(
    results.violations.filter((v) => v.impact === "serious" || v.impact === "critical"),
  ).toEqual([]);
  const style = (prop: string) =>
    dismiss.evaluate((el, p) => getComputedStyle(el).getPropertyValue(p), prop);
  expect(await style("outline-style")).toBe("solid");
  expect(await style("outline-width")).toBe("2px");
  expect(await style("outline-color")).toBe("rgb(255, 255, 255)");
  expect(await style("transition-duration")).toBe("0s");
  expect(await style("box-shadow")).toBe("none");
  // Keyboard focus (the kiosk guards keep Tab for the app, so focus it after a key press).
  await page.keyboard.press("Shift");
  await dismiss.focus();
  await expect(dismiss).toBeFocused();
  expect(await dismiss.evaluate((el) => el.matches(":focus-visible"))).toBe(true);
  expect(await style("outline-width")).toBe("4px");
  expect(await style("outline-color")).toBe("rgb(255, 225, 77)");
  const toast = page.getByTestId("toast");
  expect(await toast.evaluate((el) => getComputedStyle(el).boxShadow)).toBe("none");
});
