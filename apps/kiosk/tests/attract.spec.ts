import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { AttractView } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };

// The attract screen (Story 6.7): branding, the PC's rate and the active rule from the service's
// cache, the curated media cycling behind them, and the login panel on any key. The Tauri shell
// is stubbed in the page; the test emits its `attract` event exactly as the shell does.

type Shown = { view: AttractView | null; logo: string | null };

const sha = (c: string) => c.repeat(64);
const INR = {
  currency: "INR",
  exponent: 2,
  symbol: "₹",
  symbol_position: "before",
  number_format: "indian",
  tz: "Asia/Kolkata",
  day_start_min: 360,
  fy_start_month: 4,
  pending: null,
} as const;
// 18:00 IST on a fixed day: the rule's end, shown in the branch zone whatever this machine's.
const SIX_PM = Date.UTC(2027, 0, 15, 12, 30);

function view(over: Partial<AttractView> = {}): AttractView {
  return {
    branch_name: "Level Up Gaming",
    device_name: "PC 1",
    zone: "Main hall",
    state: "ready",
    rate_minor: 12000,
    group_name: "Gaming PCs",
    unpaid_minor: 0,
    locale: { ...INR },
    media: [],
    ...over,
  };
}

const HAPPY: NonNullable<AttractView["rule"]> = {
  rule_id: "R1",
  name: "Happy hour",
  adjust_kind: "percent_bp",
  adjust_value: -2000,
  rate_minor: 9600,
  until: SIX_PM,
};

const SVG =
  "data:image/svg+xml;base64," +
  btoa(
    '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40" fill="white"/></svg>',
  );

const COVER_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="gray"/></svg>';

async function open(page: Page, initial: Shown | null = null, opts: { hc?: boolean } = {}) {
  if (opts.hc) {
    // What the shell's first-paint script does: `data-theme` is set before any page script.
    await page.addInitScript(() => {
      const apply = () => {
        const root = document.documentElement;
        if (root) root.dataset.theme = "hc";
        return Boolean(root);
      };
      if (!apply()) {
        const observer = new MutationObserver(() => apply() && observer.disconnect());
        observer.observe(document, { childList: true });
      }
    });
  }
  await page.addInitScript(
    ([commands, shown]: [typeof names.commands, Shown | null]) => {
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
            case commands.attract:
              return shown;
            case commands.hubStatus:
            case commands.takeRestoreState:
            case commands.reportAppState:
            case commands.heartbeat:
            case commands.adminExitStatus:
            case commands.theme:
            case commands.pcScreen:
            case commands.testSession:
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
    [names.commands, initial] as [typeof names.commands, Shown | null],
  );
  await page.goto("/");
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.attract,
  );
}

const emit = (page: Page, payload: Shown) =>
  page.evaluate(
    (p) =>
      (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
        "attract",
        p,
      ),
    payload,
  );

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

const screen = (page: Page) => page.getByTestId("attract");

test("a ready PC shows the logo slot, the branch, this group's rate and no chip", async ({
  page,
}) => {
  await open(page, { view: view(), logo: null });
  await expect(screen(page)).toBeVisible();
  await expect(page.getByTestId("attract-branch")).toHaveText("Level Up Gaming");
  await expect(page.getByTestId("attract-rate")).toHaveText("₹120.00 an hour");
  await expect(page.getByTestId("chip-rule")).toHaveCount(0);
  await expect(page.getByTestId("attract-ask")).toHaveCount(0);
  // No logo set: the Arena 360 mark sits in the logo slot, top-left.
  const slot = page.getByTestId("logo-slot");
  await expect(slot.getByRole("img", { name: en["app.name"] })).toBeVisible();
  const box = (await slot.boundingBox())!;
  expect(box.x).toBeLessThan(120);
  expect(box.y).toBeLessThan(120);
  // The locked placeholder is gone, and there is no ad anywhere.
  await expect(page.getByTestId("locked")).toHaveCount(0);
  await expect(page.locator("[data-testid*='ad-'], [class*='ad-slot'], ins, iframe")).toHaveCount(
    0,
  );
  expect(await serious(page)).toEqual([]);
});

test("a rule in effect shows the live rate and its chip with name, change and end", async ({
  page,
}) => {
  await open(page, { view: view({ rate_minor: 9600, rule: HAPPY }), logo: null });
  await expect(page.getByTestId("attract-rate")).toHaveText("₹96.00 an hour");
  const chip = page.getByTestId("chip-rule");
  await expect(chip).toHaveText(/Happy hour −20% · until 6:00\s?(am|pm)/i);
  // A rate override reads as the rate.
  await emit(page, {
    view: view({
      rate_minor: 9000,
      rule: { ...HAPPY, adjust_kind: "override_minor", adjust_value: 9000, rate_minor: 9000 },
    }),
    logo: null,
  });
  await expect(chip).toContainText("Happy hour ₹90.00 an hour · until");
  // A rule with no end shows no end time.
  await emit(page, {
    view: view({ rule: { ...HAPPY, until: undefined }, rate_minor: 9600 }),
    logo: null,
  });
  await expect(chip).toHaveText(/^\s*⚡?\s*Happy hour −20%\s*$/);
});

test("a rule or rate change on the Hub reaches the screen live, without a reload", async ({
  page,
}) => {
  await open(page, { view: view(), logo: null });
  await expect(page.getByTestId("attract-rate")).toHaveText("₹120.00 an hour");
  await page.evaluate(() => ((window as unknown as { __kept: boolean }).__kept = true));
  await emit(page, { view: view({ rate_minor: 9600, rule: HAPPY }), logo: null });
  await expect(page.getByTestId("attract-rate")).toHaveText("₹96.00 an hour");
  await expect(page.getByTestId("chip-rule")).toBeVisible();
  await emit(page, { view: view({ rate_minor: 15000 }), logo: null });
  await expect(page.getByTestId("attract-rate")).toHaveText("₹150.00 an hour");
  await expect(page.getByTestId("chip-rule")).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { __kept?: boolean }).__kept)).toBe(true);
});

test("an unassigned, maintenance or rateless PC says to ask at the counter and offers no login", async ({
  page,
}) => {
  await open(page, {
    view: view({ state: "ask_counter", rate_minor: undefined, group_name: undefined }),
    logo: null,
  });
  await expect(page.getByTestId("attract-ask")).toHaveText(en["player.attract.ask_counter"]);
  await expect(page.getByTestId("attract-rate")).toHaveCount(0);
  await expect(page.getByTestId("chip-rule")).toHaveCount(0);
  await expect(page.getByTestId("attract-branch")).toHaveText("Level Up Gaming");
  await page.keyboard.press("a");
  await page.mouse.click(300, 300);
  await expect(page.getByTestId("login-panel")).toHaveCount(0);
  expect(await serious(page)).toEqual([]);
  // The PC becoming ready brings the rate (and the login) back.
  await emit(page, { view: view(), logo: null });
  await expect(page.getByTestId("attract-rate")).toBeVisible();
  await page.keyboard.press("a");
  await expect(page.getByTestId("login-panel")).toBeVisible();
});

test("a cold start paints branding, rate and rule from the cached copy at once", async ({
  page,
}) => {
  await open(page, { view: view({ rate_minor: 9600, rule: HAPPY }), logo: SVG });
  // Nothing was emitted: the shell's cached answer is on screen.
  await expect(page.getByTestId("attract-branch")).toHaveText("Level Up Gaming");
  await expect(page.getByTestId("attract-rate")).toHaveText("₹96.00 an hour");
  await expect(page.getByTestId("chip-rule")).toBeVisible();
  const logo = page.getByTestId("logo-slot").getByRole("img", { name: "Level Up Gaming logo" });
  await expect(logo).toHaveAttribute("src", SVG);
});

test("with nothing cached yet the screen asks at the counter under the Arena 360 name", async ({
  page,
}) => {
  await open(page, null);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText(en["app.name"]);
  await expect(page.getByTestId("attract-ask")).toBeVisible();
  await expect(page.getByTestId("logo-slot").getByRole("img")).toBeVisible();
  await page.keyboard.press("a");
  await expect(page.getByTestId("login-panel")).toHaveCount(0);
});

test("the logo follows the Hub and falls back to the mark when cleared or undecodable", async ({
  page,
}) => {
  await open(page, { view: view(), logo: null });
  const slot = page.getByTestId("logo-slot");
  await emit(page, { view: view(), logo: SVG });
  const img = slot.locator("img[src^='data:image/svg+xml']");
  await expect(img).toBeVisible();
  await expect(img).toHaveAttribute("alt", "Level Up Gaming logo");
  await emit(page, { view: view(), logo: null });
  await expect(slot.getByRole("img", { name: en["app.name"] })).toBeVisible();
  // A logo the webview cannot decode never shows a broken image.
  await emit(page, { view: view(), logo: "data:image/png;base64,AAAA" });
  await expect(slot.getByRole("img", { name: en["app.name"] })).toBeVisible();
  await expect(slot.locator("img")).toHaveCount(0);
});

test("any key, click or tap opens the login panel over the media; Esc closes it", async ({
  page,
}) => {
  await open(page, { view: view(), logo: null });
  const panel = page.getByTestId("login-panel");
  await expect(panel).toHaveCount(0);
  await page.keyboard.press("x");
  await expect(panel).toBeVisible();
  await expect(
    panel.getByRole("heading", { name: en["player.attract.login.title"] }),
  ).toBeVisible();
  // The form is the panel body (Story 6.8); it opens with the cursor on the username.
  await expect(panel.getByTestId("login-username")).toBeFocused();
  // The media screen stays behind the panel.
  await expect(screen(page)).toBeVisible();
  expect(await serious(page)).toEqual([]);
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  await page.mouse.click(400, 400);
  await expect(panel).toBeVisible();
  await panel.getByRole("button", { name: en["player.attract.login.close"] }).click();
  await expect(panel).toHaveCount(0);
  // A tap (touch) opens it too.
  await page.dispatchEvent("body", "pointerdown", { pointerType: "touch" });
  await expect(panel).toBeVisible();
});

test("the login panel closes after 60 s without input, and input keeps it open", async ({
  page,
}) => {
  await page.clock.install();
  await open(page, { view: view(), logo: null });
  const panel = page.getByTestId("login-panel");
  await page.keyboard.press("x");
  await expect(panel).toBeVisible();
  await page.clock.runFor(59_000);
  await expect(panel).toBeVisible();
  // Input restarts the 60 s.
  await page.keyboard.press("Shift");
  await page.clock.runFor(59_000);
  await expect(panel).toBeVisible();
  await page.clock.runFor(2_000);
  await expect(panel).toHaveCount(0);
});

test("the admin-exit combination does not open the login panel", async ({ page }) => {
  await open(page, { view: view(), logo: null });
  await page.keyboard.press("Control+Alt+Shift+A");
  await expect(page.getByTestId("admin-exit-panel")).toBeVisible();
  await expect(page.getByTestId("login-panel")).toHaveCount(0);
});

test.describe("the curated media", () => {
  const items = [
    { video: sha("a"), cover: sha("1") },
    { video: sha("b"), cover: sha("2") },
    { video: sha("c"), cover: sha("3") },
  ];
  const requested: string[] = [];

  test.beforeEach(async ({ page }) => {
    requested.length = 0;
    // Covers load; videos fail (the kiosk media transport is Story 6.23): each falls back to its
    // cover silently.
    await page.route("**/media/*", (route) => {
      const hash = new URL(route.request().url()).pathname.split("/").pop()!;
      requested.push(hash);
      return /^[123]+$/.test(hash)
        ? route.fulfill({ status: 200, contentType: "image/svg+xml", body: COVER_SVG })
        : route.fulfill({ status: 404, body: "no" });
    });
  });

  const stillSrc = (page: Page) => page.locator("[data-component='game-media'] img.still");

  test("each item plays for 30 s, then the next, and wraps", async ({ page }) => {
    await page.clock.install();
    await open(page, { view: view({ media: items }), logo: null });
    const media = page.locator("[data-component='game-media']");
    await expect(stillSrc(page)).toHaveAttribute("src", `/media/${items[0].cover}`);
    await page.clock.runFor(29_000);
    await expect(stillSrc(page)).toHaveAttribute("src", `/media/${items[0].cover}`);
    await page.clock.runFor(1_500);
    await expect(stillSrc(page)).toHaveAttribute("src", `/media/${items[1].cover}`);
    await page.clock.runFor(30_000);
    await expect(stillSrc(page)).toHaveAttribute("src", `/media/${items[2].cover}`);
    await page.clock.runFor(30_000);
    await expect(stillSrc(page)).toHaveAttribute("src", `/media/${items[0].cover}`);
    // A failed video never shows an error: the media stays, scrim and copy over it.
    await expect(media).toHaveCount(1);
    await expect(page.getByTestId("game-media-scrim")).toBeVisible();
    await expect(page.getByTestId("attract-rate")).toBeVisible();
    expect(await serious(page)).toEqual([]);
  });

  test("a single item keeps playing and nothing else is fetched from outside the Hub path", async ({
    page,
  }) => {
    await page.clock.install();
    await open(page, { view: view({ media: [items[0]] }), logo: null });
    await expect(stillSrc(page)).toHaveAttribute("src", `/media/${items[0].cover}`);
    await page.clock.runFor(95_000);
    await expect(stillSrc(page)).toHaveAttribute("src", `/media/${items[0].cover}`);
    expect(requested.every((h) => /^[0-9a-f]{64}$/.test(h))).toBe(true);
  });

  test("with no media the screen is the solid background with its copy", async ({ page }) => {
    await open(page, { view: view({ media: [] }), logo: null });
    await expect(page.locator("[data-component='game-media']")).toHaveAttribute(
      "data-stage",
      "solid",
    );
    await expect(page.getByTestId("attract-rate")).toBeVisible();
  });

  test("a new item list restarts at the first item", async ({ page }) => {
    await page.clock.install();
    await open(page, { view: view({ media: items }), logo: null });
    await page.clock.runFor(60_000);
    await expect(stillSrc(page)).toHaveAttribute("src", `/media/${items[2].cover}`);
    await emit(page, { view: view({ media: [items[1], items[2], items[0]] }), logo: null });
    await expect(stillSrc(page)).toHaveAttribute("src", `/media/${items[1].cover}`);
  });
});

test("high contrast is solid with no media or scrim, and the copy and chip still show", async ({
  page,
}) => {
  await page.route("**/media/*", (route) =>
    route.fulfill({ status: 200, contentType: "image/svg+xml", body: COVER_SVG }),
  );
  await open(
    page,
    {
      view: view({
        rate_minor: 9600,
        rule: HAPPY,
        media: [{ video: sha("a"), cover: sha("1") }],
      }),
      logo: null,
    },
    { hc: true },
  );
  await expect(page.locator("[data-component='game-media']")).toHaveAttribute(
    "data-stage",
    "solid",
  );
  await expect(page.locator("[data-component='game-media'] img")).toHaveCount(0);
  await expect(page.getByTestId("game-media-scrim")).toHaveCount(0);
  await expect(page.getByTestId("attract-rate")).toHaveText("₹96.00 an hour");
  await expect(page.getByTestId("chip-rule")).toBeVisible();
  expect(await serious(page)).toEqual([]);
  await page.keyboard.press("x");
  await expect(page.getByTestId("login-panel")).toBeVisible();
  expect(await serious(page)).toEqual([]);
});

test("the screen uses no backdrop filter and no remote fonts or media", async ({ page }) => {
  const remote: string[] = [];
  page.on("request", (r) => {
    const u = new URL(r.url());
    if (!["127.0.0.1", "localhost"].includes(u.hostname) && u.protocol.startsWith("http"))
      remote.push(r.url());
  });
  await open(page, { view: view({ rate_minor: 9600, rule: HAPPY }), logo: SVG });
  await page.keyboard.press("x");
  const filters = await page.evaluate(() =>
    [...document.querySelectorAll("*")]
      .map((el) => getComputedStyle(el).backdropFilter)
      .filter((f) => f && f !== "none"),
  );
  expect(filters).toEqual([]);
  expect(remote).toEqual([]);
});

test("a PC that owes the counter shows Pay at the counter with the amount and takes no login", async ({
  page,
}) => {
  await open(page, { view: view({ unpaid_minor: 8000 }), logo: null });
  await expect(page.getByTestId("attract-pay-at-counter")).toHaveText(
    en["player.attract.pay_at_counter"],
  );
  await expect(page.getByTestId("attract-unpaid")).toHaveText("Unpaid ₹80.00");
  await expect(page.getByTestId("attract-rate")).toHaveCount(0);
  // Any key or tap would open the login panel on a PC that is free; this one stays held.
  await page.keyboard.press("a");
  await page.mouse.click(200, 200);
  await expect(page.getByTestId("login-panel")).toHaveCount(0);
  expect(await serious(page)).toEqual([]);

  // The counter settles it: the next attract view lifts the hold on its own.
  await emit(page, { view: view({ unpaid_minor: 0 }), logo: null });
  await expect(page.getByTestId("attract-rate")).toBeVisible();
  await expect(page.getByTestId("attract-pay-at-counter")).toHaveCount(0);
  await page.keyboard.press("a");
  await expect(page.getByTestId("login-panel")).toBeVisible();
});
