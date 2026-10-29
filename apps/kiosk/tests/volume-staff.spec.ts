import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

// Story 6.15: the volume control and Call staff in the shell's top bar. The Tauri shell is
// stubbed in the page; `__calls` records what the page asked of it.

type Options = { level?: number; callOk?: boolean };

async function openWithShell(page: Page, options: Options = {}) {
  await page.addInitScript(
    ([commands, opts]: [typeof names.commands, Options]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as unknown[];
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event: string, id: number) => {
          listeners[event] = (listeners[event] ?? []).filter((x) => x !== id);
        },
      };
      let level = opts.level ?? 50;
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
            case commands.volumeGet:
              return { ok: true, body: { percent: level } };
            case commands.volumeSet:
              level = args.percent as number;
              (w.__calls as unknown[]).push(["volume", level]);
              return { ok: true, body: { percent: level } };
            case commands.callStaff:
              (w.__calls as unknown[]).push(["call_staff"]);
              return opts.callOk === false
                ? { ok: false, code: "HUB_SLOW", body: null }
                : { ok: true, body: null };
            case commands.staffMessages:
              return [];
            case commands.pcScreen:
            case commands.attract:
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
    [names.commands, options] as [typeof names.commands, Options],
  );
  await page.goto("/");
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.pcScreen,
  );
  await page.evaluate(() =>
    (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
      "pc-screen",
      {
        locked: false,
        reason: null,
        session: { device_name: "PC 7", elapsed_ms: 0, username: "asha", remaining_ms: 3_600_000 },
      },
    ),
  );
  await expect(page.getByTestId("shell")).toBeVisible();
}

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __calls: unknown[] }).__calls);

test("the right cluster reads volume, the messages bell, Call staff, the time chip, then the account", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openWithShell(page);
  const x = async (id: string) => (await page.getByTestId(id).boundingBox())!.x;
  const volume = await x("volume-control");
  const bell = await x("messages-bell");
  const call = await x("call-staff-button");
  expect(volume).toBeLessThan(bell);
  expect(bell).toBeLessThan(call);
  expect(call).toBeLessThan(await x("time-chip"));
  expect(await x("time-chip")).toBeLessThan(await x("account-chip"));
  expect((await page.getByTestId("messages-bell").boundingBox())!.height).toBeGreaterThanOrEqual(
    44,
  );
  const button = page.getByTestId("call-staff-button");
  await expect(button).toContainText(en["player.call_staff.label"]);
  expect((await button.boundingBox())!.height).toBeGreaterThanOrEqual(44);
  const speaker = (await page.getByTestId("volume-button").boundingBox())!;
  expect(speaker.width).toBeGreaterThanOrEqual(44);
  expect(speaker.height).toBeGreaterThanOrEqual(44);
});

test("the speaker opens a slider that starts at the service's level and steps 5% by arrow key", async ({
  page,
}) => {
  await openWithShell(page, { level: 35 });
  const speaker = page.getByTestId("volume-button");
  await expect(speaker).toHaveAttribute("aria-expanded", "false");
  await expect(page.getByTestId("volume-slider")).toHaveCount(0);
  await speaker.click();
  await expect(speaker).toHaveAttribute("aria-expanded", "true");
  const slider = page.getByTestId("volume-slider");
  await expect(slider).toBeFocused();
  await expect(slider).toHaveValue("35");
  await expect(page.getByTestId("volume-value")).toHaveText("35%");
  await page.keyboard.press("ArrowRight");
  await expect(slider).toHaveValue("40");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  await expect(slider).toHaveValue("30");
  // Each step is sent at once; a burst may coalesce but the last value always lands.
  await expect.poll(async () => (await calls(page)).at(-1)).toEqual(["volume", 30]);
  const sent = (await calls(page)) as [string, number][];
  for (const [, value] of sent) expect(value % 5).toBe(0);
  await expect(page.getByTestId("volume-value")).toHaveText("30%");
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
});

test("Escape closes the slider and gives the focus back to the speaker", async ({ page }) => {
  await openWithShell(page);
  await page.getByTestId("volume-button").click();
  await expect(page.getByTestId("volume-slider")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("volume-slider")).toHaveCount(0);
  await expect(page.getByTestId("volume-button")).toBeFocused();
  await page.getByTestId("volume-button").click();
  await page.getByTestId("session-device").click();
  await expect(page.getByTestId("volume-slider")).toHaveCount(0);
});

test("the slider has a 44px thumb hit area and a border-strong track with a text fill", async ({
  page,
}) => {
  await openWithShell(page, { level: 50 });
  await page.getByTestId("volume-button").click();
  const slider = page.getByTestId("volume-slider");
  expect((await slider.boundingBox())!.height).toBeGreaterThanOrEqual(44);
  // Chromium does not report slider pseudo-elements through getComputedStyle, so read the rules.
  const css = await page.evaluate(() => {
    const rules = [...document.styleSheets]
      .flatMap((s) => [...s.cssRules])
      .filter((r): r is CSSStyleRule => r instanceof CSSStyleRule);
    const of = (part: string) => rules.find((r) => r.selectorText.includes(part))?.style;
    const thumb = of("-webkit-slider-thumb");
    const track = of("-webkit-slider-runnable-track");
    return {
      touchMin: getComputedStyle(document.documentElement).getPropertyValue("--space-touch-min"),
      thumb: [thumb?.width, thumb?.height],
      track: track?.cssText ?? "",
    };
  });
  expect(css.touchMin.trim()).toBe("44px");
  expect(css.thumb).toEqual(["var(--space-touch-min)", "var(--space-touch-min)"]);
  expect(css.track).toContain("--volume-control-track");
  expect(css.track).toContain("--volume-control-fill");
});

test("Call staff sends once per press and the button says Staff notified", async ({ page }) => {
  await openWithShell(page);
  const button = page.getByTestId("call-staff-button");
  await button.click();
  await expect(page.getByTestId("call-staff-label")).toHaveText(en["player.call_staff.notified"]);
  expect(await calls(page)).toEqual([["call_staff"]]);
  // Pressing it again sends again (the Hub keeps one alert open) and still says so.
  await button.click();
  await expect.poll(async () => (await calls(page)).length).toBe(2);
  await expect(page.getByTestId("call-staff-label")).toHaveText(en["player.call_staff.notified"]);
});

test("when the Hub does not take the call the button keeps its label", async ({ page }) => {
  await openWithShell(page, { callOk: false });
  await page.getByTestId("call-staff-button").click();
  await expect.poll(async () => (await calls(page)).length).toBe(1);
  await expect(page.getByTestId("call-staff-label")).toHaveText(en["player.call_staff.label"]);
});
