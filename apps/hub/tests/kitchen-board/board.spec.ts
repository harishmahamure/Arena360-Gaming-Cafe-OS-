import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";
import type { KitchenBoard, KitchenBoardTicket } from "@arena/ui/generated";

// The kitchen board (Story 7.7), in a browser against the built Kitchen app. The Hub is a
// `routeWebSocket` stub that answers `hello` and `kitchen.board` and pushes boards.

const DISPLAY = "01JKITCHENDISPLAY00000000A";
const TOKEN = `${DISPLAY}.secret`;
const MIN = 60_000;
// The Hub's clock; the tablet's own clock is set to a different day on purpose.
const HUB_NOW = Date.UTC(2026, 9, 29, 14, 30, 0);

function ticket(
  number: number,
  stage: string,
  over: Partial<KitchenBoardTicket> = {},
): KitchenBoardTicket {
  return {
    id: `T${number}`,
    number,
    seat: `PC ${String(number).padStart(2, "0")}`,
    stage,
    placed_at: HUB_NOW - 5 * MIN,
    stage_at: HUB_NOW - 5 * MIN,
    rev: 1,
    lines: [{ line_id: `L${number}`, label: "Burger", qty: 2, notes: ["No onion"] }],
    ...over,
  };
}

function board(tickets: KitchenBoardTicket[]): KitchenBoard {
  return { branch_name: "Arena Indiranagar", now_ms: HUB_NOW, tickets };
}

type Stub = {
  sent: Record<string, unknown>[];
  push: (event: Record<string, unknown>) => void;
  revoke: () => void;
  drop: () => void;
};

/** Stands in for the Hub's `/ws`: hello, the board request, the display's own high contrast. */
async function connectDisplay(page: Page, first: KitchenBoard, hc = false): Promise<Stub> {
  const stub: { route: WebSocketRoute | null; sent: Record<string, unknown>[] } = {
    route: null,
    sent: [],
  };
  await page.routeWebSocket(/\/ws$/, (ws) => {
    stub.route = ws;
    ws.onMessage((raw) => {
      const frame = JSON.parse(String(raw)) as { rid: string; kind: string };
      stub.sent.push(frame);
      const reply = (body: Record<string, unknown>) =>
        ws.send(JSON.stringify({ v: 1, rid: frame.rid, ...body }));
      if (frame.kind === "hello") {
        reply({
          kind: "hello.ok",
          class: "kds",
          device_id: DISPLAY,
          versions: { app: "0" },
          primary: true,
        });
      } else if (frame.kind === "kitchen.board") {
        reply({ kind: "kitchen.board_result", ...first });
      } else if (frame.kind === "settings.get") {
        reply({
          kind: "settings.values",
          values: [{ key: `display.hc.${DISPLAY}`, value: String(hc), rev: 1 }],
        });
      }
    });
  });
  await page.addInitScript((token) => localStorage.setItem("arena.kitchen.token", token), TOKEN);
  return {
    sent: stub.sent,
    push: (event) => stub.route?.send(JSON.stringify({ v: 1, rid: "ev-1", kind: "event", event })),
    drop: () => void stub.route?.close(),
    revoke: () => {
      stub.route?.send(
        JSON.stringify({
          v: 1,
          rid: "h",
          kind: "error",
          error: { code: "TOKEN_REVOKED", message: "revoked" },
        }),
      );
    },
  };
}

const column = (page: Page, name: string) => page.locator(`[data-column="${name}"]`);

test.beforeEach(async ({ page }) => {
  // The tablet's clock is years away from the Hub's: elapsed times must not follow it.
  await page.clock.install({ time: new Date("2031-01-01T03:00:00Z") });
});

test("the board shows the top bar over four columns with counts, in tablet landscape", async ({
  page,
}) => {
  await connectDisplay(
    page,
    board([
      ticket(4, "new", { placed_at: HUB_NOW - 2 * MIN }),
      ticket(3, "preparing", { placed_at: HUB_NOW - 9 * MIN }),
      ticket(2, "ready", { placed_at: HUB_NOW - 4 * MIN }),
      ticket(1, "served", { placed_at: HUB_NOW - 20 * MIN, stage_at: HUB_NOW - 3 * MIN }),
    ]),
  );
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-board")).toBeVisible();

  const bar = page.getByTestId("kitchen-top-bar");
  expect((await bar.boundingBox())?.height).toBe(64);
  await expect(page.getByTestId("board-branch")).toHaveText("Arena Indiranagar");
  await expect(page.getByTestId("board-open")).toHaveText("3 open · oldest 9 min");
  await expect(bar.getByText("Connected")).toBeVisible();

  for (const [name, count] of [
    ["new", "1"],
    ["preparing", "1"],
    ["ready", "1"],
    ["served", "1"],
  ] as const) {
    await expect(column(page, name).locator(`[data-count="${name}"]`)).toContainText(count);
  }
  const heads = await page.locator("[data-column] h2").allInnerTexts();
  expect(heads.map((h) => h.split("\n")[0])).toEqual(["New", "Preparing", "Ready", "Served"]);

  // The board uses the kitchen spacing tokens: gutter 24, column gap 24.
  const boxes = await Promise.all(
    ["new", "preparing"].map(async (n) => (await column(page, n).boundingBox())!),
  );
  expect(boxes[0].x).toBe(24);
  expect(boxes[1].x - (boxes[0].x + boxes[0].width)).toBe(24);
});

test("a ticket shows the seat, number, lines with notes and the Hub's elapsed time, never a player", async ({
  page,
}) => {
  await connectDisplay(
    page,
    board([ticket(12, "new", { seat: "TV 3", placed_at: HUB_NOW - (5 * MIN + 7_000) })]),
  );
  await page.goto("/kitchen/");
  const t = page.getByTestId("kitchen-ticket");
  await expect(t.getByTestId("ticket-seat")).toHaveText("TV 3");
  await expect(t.getByTestId("ticket-number")).toHaveText("K-12");
  await expect(t.getByText("2× Burger")).toBeVisible();
  await expect(t.getByText("No onion")).toBeVisible();
  // Counted on the Hub's clock although the tablet's date is years later.
  await expect(t.getByTestId("ticket-elapsed")).toHaveText(/^5:0[7-9]$/);
  await page.clock.runFor(3_000);
  await expect(t.getByTestId("ticket-elapsed")).toHaveText(/^5:1[0-2]$/);
  await expect(t).not.toContainText(/player/i);

  // The note sits indented under its line, in muted ink.
  const line = await t.getByText("2× Burger").boundingBox();
  const note = await t.getByText("No onion").boundingBox();
  expect(note!.x).toBeGreaterThan(line!.x);
  expect(note!.y).toBeGreaterThan(line!.y);
});

test("Ready has the READY tag and a green edge; Served is muted", async ({ page }) => {
  await connectDisplay(
    page,
    board([ticket(2, "ready"), ticket(1, "served", { stage_at: HUB_NOW - MIN }), ticket(3, "new")]),
  );
  await page.goto("/kitchen/");
  const ready = column(page, "ready").getByTestId("kitchen-ticket");
  await expect(ready.getByTestId("ticket-ready")).toHaveText("Ready");
  const edge = await ready.evaluate((el) => getComputedStyle(el).borderLeftColor);
  const plain = await column(page, "new")
    .getByTestId("kitchen-ticket")
    .evaluate((el) => getComputedStyle(el).borderLeftColor);
  expect(edge).not.toBe(plain);
  await expect(column(page, "new").getByTestId("ticket-ready")).toHaveCount(0);
  const muted = await column(page, "served")
    .getByTestId("kitchen-ticket")
    .evaluate((el) => getComputedStyle(el).color);
  const ink = await column(page, "new")
    .getByTestId("kitchen-ticket")
    .evaluate((el) => getComputedStyle(el).color);
  expect(muted).not.toBe(ink);
});

test("tickets are newest first and an empty column shows its header only", async ({ page }) => {
  await connectDisplay(
    page,
    board([
      ticket(1, "new", { placed_at: HUB_NOW - 9 * MIN }),
      ticket(3, "new", { placed_at: HUB_NOW - MIN }),
      ticket(2, "new", { placed_at: HUB_NOW - 4 * MIN }),
    ]),
  );
  await page.goto("/kitchen/");
  await expect(column(page, "new").getByTestId("ticket-number")).toHaveText(["K-3", "K-2", "K-1"]);
  await expect(column(page, "ready").getByTestId("kitchen-ticket")).toHaveCount(0);
  await expect(column(page, "ready").locator("h2")).toBeVisible();
  await expect(column(page, "ready").locator('[data-count="ready"]')).toContainText("0");
});

test("a column with many tickets scrolls on its own", async ({ page }) => {
  const many = Array.from({ length: 14 }, (_, i) =>
    ticket(i + 1, "new", { placed_at: HUB_NOW - (i + 1) * 10_000 }),
  );
  await connectDisplay(page, board(many));
  await page.goto("/kitchen/");
  const stack = column(page, "new").locator(".stack");
  await expect(column(page, "new").getByTestId("kitchen-ticket")).toHaveCount(14);
  const [scroll, client] = await stack.evaluate((el) => [el.scrollHeight, el.clientHeight]);
  expect(scroll).toBeGreaterThan(client);
  const page_ = await page.evaluate(() => [
    document.documentElement.scrollHeight,
    window.innerHeight,
  ]);
  expect(page_[0]).toBeLessThanOrEqual(page_[1]);
});

test("a served ticket leaves 30 minutes after it was served", async ({ page }) => {
  await connectDisplay(
    page,
    board([ticket(1, "served", { stage_at: HUB_NOW - 29 * MIN - 50_000 })]),
  );
  await page.goto("/kitchen/");
  await expect(column(page, "served").getByTestId("kitchen-ticket")).toHaveCount(1);
  await page.clock.runFor(5_000);
  await expect(column(page, "served").getByTestId("kitchen-ticket")).toHaveCount(1);
  await page.clock.runFor(10_000);
  await expect(page.getByTestId("kitchen-ticket")).toHaveCount(0);
});

test("a cancelled ticket greys out, shows CANCELLED, cannot be acted on and leaves after 60 s", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "preparing"), ticket(2, "preparing")]));
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-ticket")).toHaveCount(2);
  stub.push({
    kind: "kitchen.board",
    ...board([
      ticket(1, "preparing"),
      ticket(2, "cancelled", {
        stage_at: HUB_NOW,
        lines: [{ line_id: "L2", label: "Burger", qty: 2, notes: [] }],
      }),
    ]),
    now_ms: HUB_NOW + 1_000,
  });
  const cancelled = page.locator('[data-ticket="T2"]');
  await expect(cancelled).toHaveAttribute("data-stage", "cancelled");
  await expect(cancelled.getByText("Cancelled")).toBeVisible();
  await expect(cancelled.getByText("2× Burger")).toHaveCSS("text-decoration-line", "line-through");
  // It stays in the column it was in, and the board has no cancel (or any) control.
  await expect(column(page, "preparing").locator('[data-ticket="T2"]')).toHaveCount(1);
  await expect(page.getByRole("button")).toHaveCount(0);
  await expect(page.getByTestId("board-open")).toHaveText("1 open · oldest 5 min");

  await page.clock.runFor(61_000);
  await expect(cancelled).toHaveCount(0);
  await expect(page.locator('[data-ticket="T1"]')).toHaveCount(1);
});

test("new tickets and stage changes are announced in a polite live region, one h1, labelled regions", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "new")]));
  await page.goto("/kitchen/");
  const live = page.getByTestId("board-announce");
  await expect(live).toHaveAttribute("aria-live", "polite");
  await expect(live).toHaveText("");

  stub.push({
    kind: "kitchen.board",
    ...board([ticket(2, "new", { seat: "Counter" }), ticket(1, "new")]),
  });
  await expect(live).toHaveText("New ticket K-2 for Counter");
  stub.push({
    kind: "kitchen.board",
    ...board([ticket(2, "new", { seat: "Counter" }), ticket(1, "ready")]),
  });
  await expect(live).toHaveText("Ticket K-1 is now Ready");

  await expect(page.getByRole("heading", { level: 1 })).toHaveCount(1);
  for (const name of ["New", "Preparing", "Ready", "Served"]) {
    await expect(page.getByRole("region", { name })).toBeVisible();
  }
});

test("tickets take keyboard focus and are at least 44px tall", async ({ page }) => {
  await connectDisplay(page, board([ticket(1, "new"), ticket(2, "ready")]));
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-board")).toBeVisible();
  await page.keyboard.press("Tab");
  const first = page.getByTestId("kitchen-ticket").first();
  await expect(first).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.getByTestId("kitchen-ticket").nth(1)).toBeFocused();
  for (const box of await page.getByTestId("kitchen-ticket").all()) {
    expect((await box.boundingBox())!.height).toBeGreaterThanOrEqual(44);
  }
});

test("high contrast uses 2px outlines and the yellow focus ring", async ({ page }) => {
  await connectDisplay(page, board([ticket(1, "new")]), true);
  await page.goto("/kitchen/");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "hc");
  const t = page.getByTestId("kitchen-ticket");
  await expect(t).toHaveCSS("border-top-width", "2px");
  await page.keyboard.press("Tab");
  await expect(t).toBeFocused();
  await expect(t).toHaveCSS("outline-style", "solid");
  await expect(t).toHaveCSS("outline-color", "rgb(255, 225, 77)");
});

test("the board shows Offline when the Hub is lost and keeps its tickets", async ({ page }) => {
  const stub = await connectDisplay(page, board([ticket(1, "new")]));
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-top-bar").getByText("Connected")).toBeVisible();
  stub.drop();
  await expect(page.getByTestId("kitchen-top-bar").getByText("Offline")).toBeVisible();
  await expect(page.getByTestId("kitchen-ticket")).toHaveCount(1);
  // The page reconnects by itself and asks for the board again.
  await page.clock.runFor(2_500);
  await expect(page.getByTestId("kitchen-top-bar").getByText("Connected")).toBeVisible();
});

test("a revoked token returns to the pairing screen", async ({ page }) => {
  const stub = await connectDisplay(page, board([ticket(1, "new")]));
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-board")).toBeVisible();
  stub.revoke();
  await expect(page.getByTestId("kitchen-pair")).toBeVisible();
  await expect(page.getByTestId("kitchen-board")).toHaveCount(0);
});

test("the board has no accessibility violations", async ({ page }) => {
  await connectDisplay(
    page,
    board([ticket(3, "new"), ticket(2, "ready"), ticket(1, "served", { stage_at: HUB_NOW - MIN })]),
  );
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-board")).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});
