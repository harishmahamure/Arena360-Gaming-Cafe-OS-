import AxeBuilder from "@axe-core/playwright";
import { expect, type Page } from "@playwright/test";
import names from "../src-tauri/ipc-names.json" with { type: "json" };

// Shared by the Story 6.37 suites (reflow, keyboard journey, high contrast): one stubbed Tauri
// shell that answers every kiosk command from `window.__service`, so a test can walk the whole
// player journey. A test changes an answer with `answer()`; `emit()` plays a shell event.

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
};

export const ATTRACT = {
  view: {
    branch_name: "Level Up Gaming",
    device_name: "PC 7",
    zone: "Main hall",
    state: "ready",
    rate_minor: 12000,
    group_name: "Gaming PCs",
    unpaid_minor: 0,
    locale: INR,
    media: [],
  },
  logo: null,
};

const CATEGORIES = ["Shooter", "Racing", "Sports", "Strategy"];
const NAMES = [
  "Apex Legends",
  "Valorant",
  "Counter-Strike 2",
  "Fortnite",
  "Rocket League",
  "EA Sports FC 25",
  "Dota 2",
  "League of Legends",
  "Forza Horizon 5",
  "Minecraft",
  "GTA V",
  "Rainbow Six Siege",
  "Overwatch 2",
  "Call of Duty Warzone",
  "Tekken 8",
  "Street Fighter 6",
  "Age of Empires IV",
  "Marvel Rivals",
  "Delta Force",
  "Rust",
];

const GAMES = NAMES.map((name, i) => ({
  id: `G${i + 1}`,
  name,
  running: false,
  category: CATEGORIES[i % CATEGORIES.length],
  tags: i % 3 === 0 ? ["Multiplayer"] : ["Co-op"],
  launcher: ["steam", "epic", "riot"][i % 3],
  age_rating: "PEGI 16",
}));

const APPS = ["Chrome", "Discord", "OBS Studio", "Spotify"].map((name, i) => ({
  id: `A${i + 1}`,
  name,
  running: false,
}));

const item = (id: string, name: string, extra: Record<string, unknown> = {}) => ({
  id,
  name,
  category: "Drinks",
  price_minor: 4000,
  bundle: false,
  available: true,
  has_image: false,
  popular: false,
  components: [],
  ...extra,
});

export const MENU = {
  enabled: true,
  items: [
    item("B1", "Gamer combo", {
      price_minor: 9000,
      bundle: true,
      components: [
        { name: "Cola", qty: 2 },
        { name: "Chips", qty: 1 },
      ],
    }),
    item("P1", "Cola"),
    item("P2", "Iced tea", { available: false, max_qty: 0 }),
    item("P4", "Chips", { category: "Snacks" }),
    item("P5", "Burger", { category: "Snacks", price_minor: 15000 }),
  ],
  pay: { bill: true, wallet_minor: 25_050, counter: true },
};

export const ACCOUNT_PAGE = {
  name: "Asha Rao",
  username: "asha",
  phone_masked: "+••••••••3210",
  balance_minor: 20_000,
  packs: [
    {
      name: "5 h",
      groups: ["PC", "VR"],
      minutes_left: 135,
      expires_at: Date.UTC(2027, 10, 20),
      expired: false,
    },
  ],
  orders: [
    {
      order_id: "O2",
      at: Date.UTC(2026, 9, 28, 14, 30),
      items: [
        { name: "Cola", qty: 2 },
        { name: "Chips", qty: 1 },
      ],
      total_minor: 12_000,
      status: "settled",
      invoice_no: "MUM/26-27/E1/7",
    },
  ],
};

const FUTURE = Date.UTC(2027, 2, 4, 6, 30);

export const PLANS = {
  ok: true,
  body: {
    wallet: { balance_minor: 4000, rate_minor: 12000 },
    options: [
      {
        choice: { mode: "pack", pack_id: "K1" },
        name: "10 hour pack",
        selectable: true,
        minutes_left: 150,
        expires_at: FUTURE,
      },
      {
        choice: { mode: "pass", pack_id: "S1" },
        name: "Night pass",
        selectable: true,
        expires_at: FUTURE,
        window: "22:00–06:00",
      },
      {
        choice: { mode: "block", plan_id: "B1" },
        name: "Night block",
        selectable: true,
        window: "22:00–06:00",
        ends_label: "06:00",
        price_minor: 20000,
      },
      { choice: { mode: "wallet" }, name: "", selectable: true },
    ],
  },
};

const ok = (body: unknown) => ({ ok: true, body });

const DEFAULTS: Record<string, unknown> = {
  [names.commands.attract]: ATTRACT,
  [names.commands.hubStatus]: { state: "connected" },
  [names.commands.playerLogin]: ok({
    outcome: "ok",
    player: { player_id: "P1", name: "Asha Rao" },
  }),
  [names.commands.playerSignup]: null,
  [names.commands.playerPlans]: PLANS,
  [names.commands.playerStart]: ok({ outcome: "ok" }),
  [names.commands.playerEnd]: ok({ outcome: "ok" }),
  [names.commands.playerLogout]: null,
  [names.commands.playerAccountPage]: ok(ACCOUNT_PAGE),
  [names.commands.games]: ok({ games: GAMES, running: false }),
  [names.commands.gameLaunch]: ok({ outcome: "started", pid: 4242 }),
  [names.commands.apps]: ok({ apps: APPS }),
  [names.commands.launchers]: ok({
    launchers: [
      { id: "steam", name: "Steam", running: false },
      { id: "epic", name: "Epic Games", running: false },
    ],
  }),
  [names.commands.mostPlayed]: ok({
    top: GAMES.slice(0, 10).map((g, i) => ({ game_id: g.id, minutes: 600 - i * 60 })),
    last_played: [],
  }),
  [names.commands.shopMenu]: ok(MENU),
  [names.commands.shopImage]: ok({ id: "x", image: null }),
  [names.commands.shopPlace]: ok({ outcome: "ok" }),
  [names.commands.volumeGet]: ok({ percent: 50 }),
  [names.commands.staffMessages]: [],
};

/** Opens `path` with the stubbed shell; `service` overrides the default answers by command. */
export async function openKiosk(
  page: Page,
  { path = "/", service = {} }: { path?: string; service?: Record<string, unknown> } = {},
) {
  await page.addInitScript(
    ([defaults, over]: [Record<string, unknown>, Record<string, unknown>]) => {
      const callbacks = new Map<number, (e: unknown) => void>();
      const listeners: Record<string, number[]> = {};
      let next = 1;
      const w = window as unknown as Record<string, unknown>;
      w.__calls = [] as string[];
      w.__service = { ...defaults, ...over };
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
            default:
              (w.__calls as string[]).push(cmd);
              return (w.__service as Record<string, unknown>)[cmd] ?? null;
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
    [DEFAULTS, service] as [Record<string, unknown>, Record<string, unknown>],
  );
  await page.goto(path);
  const events = path === "/hud" ? [names.events.pcScreen] : [names.events.hubStatus];
  for (const event of events) {
    await page.waitForFunction(
      (e) =>
        (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
          e,
        ),
      event,
    );
  }
}

export const emit = (page: Page, event: string, payload: unknown = null) =>
  page.evaluate(
    ([e, p]) =>
      (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
        e as string,
        p,
      ),
    [event, payload] as const,
  );

/** Changes what the stubbed shell answers to `command` from now on. */
export const answer = (page: Page, command: string, value: unknown) =>
  page.evaluate(
    ([c, v]) =>
      ((window as unknown as { __service: Record<string, unknown> }).__service[c as string] = v),
    [command, value] as const,
  );

export const shown = (session: Record<string, unknown> | null, over: object = {}) => ({
  locked: false,
  reason: null,
  session: session && { device_name: "PC 7", elapsed_ms: 0, ...session },
  ...over,
});

export const PREPAID = {
  username: "asha",
  remaining_ms: 6_120_000,
  balance_minor: 25_050,
  grace_ms: 120_000,
  warning_min: null,
};

/** Opens the shell for a profiled prepaid player on the stubbed shell. */
export async function openShell(page: Page, tab?: string) {
  await openKiosk(page);
  await emit(page, names.events.attract, ATTRACT);
  await emit(page, names.events.pcScreen, shown(PREPAID));
  await expect(page.getByTestId("shell")).toBeVisible();
  if (tab) await page.getByTestId(`tab-${tab}`).click();
}

export const serious = async (page: Page) =>
  (await new AxeBuilder({ page }).analyze()).violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );

/** The four widths of Story 6.37: 1366, 1920, 2560 and a 21:9 ultrawide. */
export const WIDTHS = [
  { name: "1366", width: 1366, height: 768 },
  { name: "1920", width: 1920, height: 1080 },
  { name: "2560", width: 2560, height: 1440 },
  { name: "ultrawide", width: 3440, height: 1440 },
] as const;

/**
 * Lengthens every text on the page by 30% (a pseudo-locale), so a longer translation is tried
 * without a second catalogue. Text that is a number or a symbol is left as it is.
 */
export const pseudoLocale = (page: Page) =>
  page.evaluate(() => {
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    const nodes: Text[] = [];
    for (let n = walker.nextNode(); n; n = walker.nextNode()) nodes.push(n as Text);
    for (const node of nodes) {
      const text = node.nodeValue ?? "";
      const parent = node.parentElement;
      if (!parent || ["SCRIPT", "STYLE"].includes(parent.tagName)) continue;
      if (!/[A-Za-z]{3}/.test(text)) continue;
      const extra = Math.ceil(text.trim().length * 0.3);
      node.nodeValue = text + " " + "ēxtra ".repeat(Math.ceil(extra / 6)).slice(0, extra);
    }
  });

type Problem = string;

/**
 * What a viewport at this width would show wrong: horizontal scrolling anywhere, a control
 * outside the screen, or two controls on top of each other.
 */
export const layoutProblems = (page: Page): Promise<Problem[]> =>
  page.evaluate(() => {
    const out: string[] = [];
    const vw = window.innerWidth;
    // The most-played carousel is a row that scrolls on purpose (arrow keys step through it).
    const inCarousel = (el: Element) => el.closest("[data-testid=most-played-row]") !== null;
    const label = (el: Element) =>
      (el.getAttribute("data-testid") ?? el.tagName.toLowerCase()) +
      (el.textContent ? `"${el.textContent.trim().slice(0, 24)}"` : "");
    const root = document.scrollingElement as HTMLElement;
    if (root.scrollWidth > vw + 1) out.push(`page scrolls sideways (${root.scrollWidth} > ${vw})`);
    for (const el of document.querySelectorAll<HTMLElement>("*")) {
      const style = getComputedStyle(el);
      if (style.display === "none" || style.visibility === "hidden") continue;
      const scrolls = style.overflowX === "auto" || style.overflowX === "scroll";
      if (scrolls && !inCarousel(el) && el.scrollWidth > el.clientWidth + 1 && el.clientWidth > 0) {
        out.push(`${label(el)} scrolls sideways (${el.scrollWidth} > ${el.clientWidth})`);
      }
    }
    // A fixed box smaller than the screen: a panel (the cart, a menu), not the kiosk window.
    const fixedParent = (el: Element) => {
      for (let p: Element | null = el; p; p = p.parentElement) {
        if (getComputedStyle(p).position !== "fixed") continue;
        const r = p.getBoundingClientRect();
        if (r.width < vw - 1 || r.height < window.innerHeight - 1) return p;
      }
      return null;
    };
    const controls = [
      ...document.querySelectorAll<HTMLElement>(
        "button, input, select, textarea, a[href], [role=slider]",
      ),
    ].filter((el) => {
      const r = el.getBoundingClientRect();
      const style = getComputedStyle(el);
      return (
        r.width > 0 &&
        r.height > 0 &&
        style.visibility !== "hidden" &&
        !el.closest("[inert]") &&
        !inCarousel(el)
      );
    });
    for (const el of controls) {
      const r = el.getBoundingClientRect();
      if (r.left < -1 || r.right > vw + 1) out.push(`${label(el)} leaves the screen`);
    }
    for (let i = 0; i < controls.length; i++) {
      for (let j = i + 1; j < controls.length; j++) {
        const a = controls[i];
        const b = controls[j];
        if (a.contains(b) || b.contains(a)) continue;
        // A fixed panel (the cart, a menu) is meant to sit over what is behind it.
        if (Boolean(fixedParent(a)) !== Boolean(fixedParent(b))) continue;
        const ra = a.getBoundingClientRect();
        const rb = b.getBoundingClientRect();
        const w = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left);
        const h = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
        if (w > 2 && h > 2) out.push(`${label(a)} overlaps ${label(b)}`);
      }
    }
    return out;
  });
