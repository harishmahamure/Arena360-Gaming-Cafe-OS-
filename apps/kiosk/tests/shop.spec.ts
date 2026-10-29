import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import {
  categoryLabels,
  hasAvailable,
  shopImageOf,
  shopMenuOf,
  visibleItems,
} from "../src/lib/shop";

// Story 6.30: the Shop tab. The Tauri shell is stubbed in the page; `__service` holds what the
// Hub would answer to `shop_menu` and `shop_image` (a test changes it to play a Hub stock change)
// and `__calls` records what the page asked of the shell.

type Part = { name: string; qty: number };
type Item = {
  id: string;
  name: string;
  category: string;
  price_minor: number;
  bundle: boolean;
  available: boolean;
  has_image: boolean;
  popular: boolean;
  components: Part[];
  kitchen_notes: string[];
};
type Menu = { enabled: boolean; items: Item[] };
type Options = { menu?: Menu | null };

const INR = {
  currency: "INR",
  exponent: 2,
  symbol: "₹",
  symbol_position: "prefix",
  number_format: "indian",
  tz: "Asia/Kolkata",
  day_start_min: 0,
  fy_start_month: 4,
  pending: null,
};

/** A JPEG data URL the page shows as a photo (a 1 x 1 picture). */
const PHOTO =
  "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=";

const item = (id: string, name: string, category: string, extra: Partial<Item> = {}): Item => ({
  id,
  name,
  category,
  price_minor: 4000,
  bundle: false,
  available: true,
  has_image: false,
  popular: false,
  components: [],
  kitchen_notes: [],
  ...extra,
});

const MENU: Menu = {
  enabled: true,
  items: [
    item("B1", "Gamer combo", "Combos", {
      price_minor: 9000,
      bundle: true,
      components: [
        { name: "Cola", qty: 2 },
        { name: "Chips", qty: 1 },
      ],
    }),
    item("P1", "Cola", "Drinks", { has_image: true, popular: true }),
    item("P2", "Iced tea", "Drinks", { available: false, has_image: true }),
    item("P3", "Maggi", "Snacks", { price_minor: 12_345, popular: true }),
    item("P4", "Chips", "Snacks"),
  ],
};

async function openShop(page: Page, options: Options = {}) {
  await page.addInitScript(
    ([commands, opts]: [typeof names.commands, Options]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as unknown[];
      w.__service = {
        menu: opts.menu === undefined ? null : opts.menu,
        photo: "",
      };
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
          const service = w.__service as { menu: unknown; photo: string };
          switch (cmd) {
            case "plugin:event|listen":
              (listeners[args.event as string] ??= []).push(args.handler as number);
              return args.handler;
            case "plugin:event|unlisten":
              return null;
            case commands.shopMenu:
              (w.__calls as unknown[]).push(["shop_menu"]);
              // `null`: the Hub cannot be asked.
              return service.menu === null
                ? { ok: false, code: "HUB_SLOW", body: null }
                : { ok: true, body: service.menu };
            case commands.shopImage:
              (w.__calls as unknown[]).push(["shop_image", args.id]);
              return {
                ok: true,
                body: { id: args.id, image: args.id === "P1" ? service.photo : null },
              };
            case commands.games:
              return {
                ok: true,
                body: { games: [{ id: "G1", name: "Apex", running: false }], running: false },
              };
            case commands.apps:
              return { ok: true, body: { apps: [] } };
            case commands.launchers:
              return { ok: true, body: { launchers: [] } };
            case commands.mostPlayed:
              return { ok: true, body: { top: [], last_played: [] } };
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
            case commands.volumeGet:
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
  for (const event of [names.events.pcScreen, names.events.attract]) {
    await page.waitForFunction(
      (e) =>
        (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
          e,
        ),
      event,
    );
  }
  await page.evaluate(
    ([photo, locale]) => {
      const w = window as unknown as {
        __service: { photo: string };
        __shell: { emit: (e: string, p: unknown) => void };
      };
      w.__service.photo = photo;
      w.__shell.emit("attract", {
        view: { branch_name: "Arena", state: "ready", locale, rate_minor: 6000, media: [] },
        logo: null,
      });
      w.__shell.emit("pc-screen", {
        locked: false,
        reason: null,
        session: { device_name: "PC 7", elapsed_ms: 0, username: "asha", remaining_ms: 3_600_000 },
      });
    },
    [PHOTO, INR] as const,
  );
  await expect(page.getByTestId("shell")).toBeVisible();
}

/** What the Hub answers to the next `shop_menu`. */
const hubSays = (page: Page, menu: Menu | null) =>
  page.evaluate((m) => {
    (window as unknown as { __service: { menu: unknown } }).__service.menu = m;
  }, menu);

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __calls: unknown[][] }).__calls);

const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

/** The card whose name is `name` (a bundle's component list names other products too). */
const card = (page: Page, name: string) =>
  page
    .getByTestId("shop-item-card")
    .filter({ has: page.getByTestId("shop-item-name").getByText(name, { exact: true }) });

test("a branch with kiosk orders shows Shop after Apps", async ({ page }) => {
  await openShop(page, { menu: MENU });
  await expect(page.getByTestId("pill-nav").getByRole("button")).toHaveText([
    en["player.shell.tab.home"],
    en["player.shell.tab.games"],
    en["player.shell.tab.apps"],
    en["player.shell.tab.shop"],
  ]);
});

test("on a Free branch there is no Shop item and the Hub's empty answer stays empty", async ({
  page,
}) => {
  await openShop(page, { menu: { enabled: false, items: [] } });
  await expect(page.getByTestId("tab-apps")).toBeVisible();
  await expect(page.getByTestId("tab-shop")).toHaveCount(0);
});

test("until the Hub has answered there is no Shop item", async ({ page }) => {
  await openShop(page, { menu: null });
  await expect(page.getByTestId("tab-apps")).toBeVisible();
  await expect(page.getByTestId("tab-shop")).toHaveCount(0);
  // The Hub answers later (it was unreachable): the Shop appears without a restart.
  await hubSays(page, MENU);
  await page.getByTestId("tab-games").click();
  await expect(page.getByTestId("tab-shop")).toBeVisible();
});

test("the Shop tab shows the filter rail, the search field and a card per item", async ({
  page,
}) => {
  await openShop(page, { menu: MENU });
  await page.getByTestId("tab-shop").click();
  await expect(page.getByTestId("shop")).toBeVisible();
  await expect(page.getByTestId("filter-item")).toHaveText([
    en["player.games.filter_all"],
    "Combos",
    "Drinks",
    "Snacks",
  ]);
  await expect(page.getByTestId("shop-search")).toHaveAttribute(
    "aria-label",
    "Search food & drinks",
  );
  await expect(page.getByTestId("shop-item-name")).toHaveText([
    "Gamer combo",
    "Cola",
    "Iced tea",
    "Maggi",
    "Chips",
  ]);
  const maggi = card(page, "Maggi");
  await expect(maggi.getByTestId("shop-item-price")).toHaveText("₹123.45");
  await expect(maggi.getByTestId("shop-item-price")).toHaveClass(/tabular-nums/);
  await expect(maggi.getByTestId("shop-item-add")).toBeEnabled();
  await expect(maggi.getByTestId("shop-item-add")).toHaveText(en["player.shop.add"]);
  expect(await serious(page)).toEqual([]);
});

test("a bundle card lists its components", async ({ page }) => {
  await openShop(page, { menu: MENU });
  await page.getByTestId("tab-shop").click();
  const combo = card(page, "Gamer combo");
  await expect(combo.getByTestId("shop-item-components").getByRole("listitem")).toHaveText([
    "2 × Cola",
    "1 × Chips",
  ]);
  await expect(card(page, "Cola").getByTestId("shop-item-components")).toHaveCount(0);
});

test("the top sellers carry the POPULAR badge, others do not", async ({ page }) => {
  await openShop(page, { menu: MENU });
  await page.getByTestId("tab-shop").click();
  await expect(page.getByTestId("popular-badge")).toHaveText([
    en["player.shop.popular"],
    en["player.shop.popular"],
  ]);
  await expect(card(page, "Cola").getByTestId("popular-badge")).toBeVisible();
  await expect(card(page, "Maggi").getByTestId("popular-badge")).toBeVisible();
  await expect(card(page, "Chips").getByTestId("popular-badge")).toHaveCount(0);
});

test("an unavailable card dims the photo, keeps its ink and disables Add", async ({ page }) => {
  await openShop(page, { menu: MENU });
  await page.getByTestId("tab-shop").click();
  const tea = card(page, "Iced tea");
  await expect(tea).toHaveAttribute("data-available", "false");
  await expect(tea.getByTestId("shop-item-oos")).toHaveText(en["player.shop.out_of_stock"]);
  await expect(tea.getByTestId("shop-item-add")).toBeDisabled();
  const opacity = (testid: string, c = tea) =>
    c.getByTestId(testid).evaluate((el) => Number(getComputedStyle(el).opacity));
  expect(await opacity("shop-item-photo")).toBeLessThan(0.5);
  expect(await opacity("shop-item-name")).toBe(1);
  expect(await opacity("shop-item-oos")).toBe(1);
  // An available card is not dimmed and has no tag.
  const cola = card(page, "Cola");
  expect(await opacity("shop-item-photo", cola)).toBe(1);
  await expect(cola.getByTestId("shop-item-oos")).toHaveCount(0);
  // The name and the tag keep their full-ink colour (not the muted one).
  const ink = (c = tea) =>
    c.getByTestId("shop-item-name").evaluate((el) => getComputedStyle(el).color);
  expect(await ink()).toBe(await ink(cola));
  expect(await serious(page)).toEqual([]);
});

test("a Hub stock change shows on the open Shop within 2 seconds", async ({ page }) => {
  await openShop(page, { menu: MENU });
  await page.getByTestId("tab-shop").click();
  const chips = card(page, "Chips");
  await expect(chips).toHaveAttribute("data-available", "true");
  const flipped = {
    ...MENU,
    items: MENU.items.map((i) => (i.id === "P4" ? { ...i, available: false } : i)),
  };
  await hubSays(page, flipped);
  await expect(chips).toHaveAttribute("data-available", "false", { timeout: 2000 });
  await expect(chips.getByTestId("shop-item-add")).toBeDisabled();
  // Back in stock: it flips back.
  await hubSays(page, MENU);
  await expect(chips).toHaveAttribute("data-available", "true", { timeout: 2000 });
  await expect(chips.getByTestId("shop-item-add")).toBeEnabled();
});

test("a product the Hub drops from the menu leaves the Shop on the next ask", async ({ page }) => {
  await openShop(page, { menu: MENU });
  await page.getByTestId("tab-shop").click();
  await expect(page.getByTestId("shop-item-card")).toHaveCount(5);
  await hubSays(page, { ...MENU, items: MENU.items.filter((i) => i.id !== "P4") });
  await expect(page.getByTestId("shop-item-card")).toHaveCount(4, { timeout: 2000 });
});

test("with nothing available the Shop says the kitchen is closed", async ({ page }) => {
  await openShop(page, {
    menu: { enabled: true, items: MENU.items.map((i) => ({ ...i, available: false })) },
  });
  await page.getByTestId("tab-shop").click();
  await expect(page.getByTestId("shop-closed")).toHaveText("Kitchen's closed right now.");
  await expect(page.getByTestId("shop-item-card")).toHaveCount(0);
  // Stock comes back: the cards return.
  await hubSays(page, MENU);
  await expect(page.getByTestId("shop-item-card")).toHaveCount(5, { timeout: 2000 });
  await expect(page.getByTestId("shop-closed")).toHaveCount(0);
});

test("an empty menu says the kitchen is closed too", async ({ page }) => {
  await openShop(page, { menu: { enabled: true, items: [] } });
  await page.getByTestId("tab-shop").click();
  await expect(page.getByTestId("shop-closed")).toBeVisible();
});

test("a category chip and the search filter the cards, and nothing matching says so", async ({
  page,
}) => {
  await openShop(page, { menu: MENU });
  await page.getByTestId("tab-shop").click();
  await page.getByTestId("filter-item").filter({ hasText: "Drinks" }).click();
  await expect(page.getByTestId("shop-item-name")).toHaveText(["Cola", "Iced tea"]);
  await page.getByTestId("shop-search").fill("COL");
  await expect(page.getByTestId("shop-item-name")).toHaveText(["Cola"]);
  await page.getByTestId("shop-search").fill("zzz");
  await expect(page.getByTestId("shop-no-match")).toContainText("Nothing matches.");
  await page.getByTestId("shop-clear").click();
  await expect(page.getByTestId("shop-item-card")).toHaveCount(5);
  await expect(page.getByTestId("shop-search")).toHaveValue("");
});

test("photos are asked of the Hub once per product and shown", async ({ page }) => {
  await openShop(page, { menu: MENU });
  await page.getByTestId("tab-shop").click();
  await expect(card(page, "Cola").locator("img")).toHaveAttribute("src", PHOTO);
  // The Iced tea has a photo on the Hub's menu but the Hub holds none: no image, no retry.
  await expect(card(page, "Iced tea").locator("img")).toHaveCount(0);
  // Let several polls pass: no photo is asked twice.
  await page.waitForTimeout(2500);
  const asked = (await calls(page)).filter((c) => c[0] === "shop_image").map((c) => c[1]);
  expect(asked.sort()).toEqual(["P1", "P2"]);
  expect((await calls(page)).filter((c) => c[0] === "shop_menu").length).toBeGreaterThan(2);
  // Items without a photo are never asked for one.
  await page.getByTestId("tab-games").click();
  await page.getByTestId("tab-shop").click();
  expect((await calls(page)).filter((c) => c[0] === "shop_image")).toHaveLength(2);
});

test("the menu is asked every second while Shop is open and rarely otherwise", async ({ page }) => {
  await openShop(page, { menu: MENU });
  await page.waitForTimeout(1800);
  const idle = (await calls(page)).filter((c) => c[0] === "shop_menu").length;
  // Home is open: the first ask only (the next is 30 s away).
  expect(idle).toBe(1);
  await page.getByTestId("tab-shop").click();
  await page.waitForTimeout(2600);
  const open = (await calls(page)).filter((c) => c[0] === "shop_menu").length;
  expect(open).toBeGreaterThanOrEqual(idle + 3);
});

test("a Shop tab is replaced by Home when the Hub turns orders off", async ({ page }) => {
  await openShop(page, { menu: MENU });
  await page.getByTestId("tab-shop").click();
  await expect(page.getByTestId("shop")).toBeVisible();
  await hubSays(page, { enabled: false, items: [] });
  await expect(page.getByTestId("tab-shop")).toHaveCount(0, { timeout: 2000 });
  await expect(page.getByTestId("shop")).toHaveCount(0);
  await expect(page.getByTestId("tab-home")).toHaveAttribute("aria-current", "page");
});

test("when the Hub cannot be asked the last good menu stays and the Shop item stays", async ({
  page,
}) => {
  await openShop(page, { menu: MENU });
  await page.getByTestId("tab-shop").click();
  await expect(page.getByTestId("shop-item-card")).toHaveCount(5);
  await hubSays(page, null);
  await page.waitForTimeout(2500);
  await expect(page.getByTestId("shop-item-card")).toHaveCount(5);
  await expect(page.getByTestId("tab-shop")).toBeVisible();
});

test("arrow keys step through the rail", async ({ page }) => {
  await openShop(page, { menu: MENU });
  await page.getByTestId("tab-shop").click();
  const rail = page.getByTestId("filter-item");
  await rail.nth(0).focus();
  await page.keyboard.press("ArrowDown");
  await expect(rail.nth(1)).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("shop-item-name")).toHaveText(["Gamer combo"]);
});

test("the answers are read defensively", () => {
  const good = { enabled: true, items: [item("P1", "Cola", "Drinks")] };
  // An older Hub says nothing about payment: the session bill is the one choice (Story 6.32).
  const billOnly = { bill: true, counter: false };
  expect(shopMenuOf(good)).toEqual({ ...good, pay: billOnly });
  expect(
    shopMenuOf({ ...good, pay: { bill: false, wallet_minor: 500, counter: true } })?.pay,
  ).toEqual({
    bill: false,
    wallet_minor: 500,
    counter: true,
  });
  // A branch without the feature has no menu, whatever the answer carries.
  expect(shopMenuOf({ enabled: false, items: good.items })).toEqual({
    enabled: false,
    items: [],
    pay: billOnly,
  });
  for (const bad of [
    null,
    "x",
    {},
    { enabled: true },
    { enabled: "yes", items: [] },
    { enabled: true, items: [{ id: "P1" }] },
    { enabled: true, items: [{ ...item("P1", "Cola", "Drinks"), components: [{ name: 1 }] }] },
  ]) {
    expect(shopMenuOf(bad)).toBeNull();
  }
  expect(shopImageOf({ id: "P1", image: PHOTO }, "P1")).toBe(PHOTO);
  expect(shopImageOf({ id: "P1", image: null }, "P1")).toBeNull();
  // Only a JPEG data URL for the product asked about is a photo.
  expect(shopImageOf({ id: "P1", image: "https://example.com/a.jpg" }, "P1")).toBeNull();
  expect(shopImageOf({ id: "P2", image: PHOTO }, "P1")).toBeUndefined();
  expect(shopImageOf(null, "P1")).toBeUndefined();
});

test("the rules: categories, filters and the closed kitchen", () => {
  expect(categoryLabels([...MENU.items, item("P9", "Odd", " drinks ")])).toEqual([
    "Combos",
    "Drinks",
    "Snacks",
  ]);
  expect(visibleItems(MENU.items, "snacks", "").map((i) => i.id)).toEqual(["P3", "P4"]);
  expect(visibleItems(MENU.items, null, " CHI ").map((i) => i.id)).toEqual(["P4"]);
  expect(hasAvailable([])).toBe(false);
  expect(hasAvailable(MENU.items.map((i) => ({ ...i, available: false })))).toBe(false);
  expect(hasAvailable(MENU.items)).toBe(true);
});
