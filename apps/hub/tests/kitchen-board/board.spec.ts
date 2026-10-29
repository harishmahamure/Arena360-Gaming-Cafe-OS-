import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";
import type { KitchenBoard, KitchenBoardTicket } from "@arena/ui/generated";

// The kitchen board (Story 7.7), its one-tap advance with Undo (Story 7.8) and its work through
// LAN loss (Story 7.10), in a browser against the built Kitchen app. The Hub is a
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
  /** Makes the Hub refuse intents of this kind (of one ticket, if given) with this error code (null accepts again). */
  refuse: (kind: string, code: string | null, ticketId?: string) => void;
  /** The board the Hub answers the next request with, without pushing it. */
  setBoard: (next: KitchenBoard) => void;
  /** Takes the Hub off the LAN (it closes every new connection) or brings it back. */
  reachable: (up: boolean) => void;
  /** Whether the Hub answers requests at all (false: it takes them and says nothing). */
  silent: (on: boolean) => void;
  /** How many intents of each idempotency key the Hub received (it applies a key once). */
  received: Map<string, number>;
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
  // The Hub answers a board request with its latest board, the last one it pushed.
  let current = first;
  const refusals = new Map<string, string>();
  const received = new Map<string, number>();
  let up = true;
  let mute = false;
  await page.routeWebSocket(/\/ws$/, (ws) => {
    if (!up) {
      void ws.close();
      return;
    }
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
        reply({ kind: "kitchen.board_result", ...current });
      } else if (frame.kind === "intent.submit") {
        const intent = (
          frame as unknown as {
            intent: {
              kind: string;
              intent_id: string;
              idempotency_key: string;
              payload: { ticket_id?: string };
            };
          }
        ).intent;
        received.set(intent.idempotency_key, (received.get(intent.idempotency_key) ?? 0) + 1);
        if (mute) return;
        const code =
          refusals.get(`${intent.kind}:${intent.payload.ticket_id}`) ?? refusals.get(intent.kind);
        if (code) reply({ kind: "error", error: { code, message: code } });
        else
          reply({ kind: "intent.result", applied: { intent_id: intent.intent_id, changes: [] } });
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
    refuse: (kind, code, ticketId) => {
      const at = ticketId ? `${kind}:${ticketId}` : kind;
      if (code) refusals.set(at, code);
      else refusals.delete(at);
    },
    setBoard: (next) => void (current = next),
    reachable: (on) => void (up = on),
    silent: (on) => void (mute = on),
    received,
    push: (event) => {
      if (event.kind === "kitchen.board") current = event as unknown as KitchenBoard;
      stub.route?.send(JSON.stringify({ v: 1, rid: "ev-1", kind: "event", event }));
    },
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
  await expect(cancelled).not.toHaveAttribute("role", "button");
  await expect(page.locator('[data-testid="kitchen-ticket"][role="button"]')).toHaveCount(1); // only the Preparing ticket takes a tap
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
  await page.getByTestId("chime-toggle").focus();
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
  await page.getByTestId("chime-toggle").focus();
  await page.keyboard.press("Tab");
  await expect(t).toBeFocused();
  await expect(t).toHaveCSS("outline-style", "solid");
  await expect(t).toHaveCSS("outline-color", "rgb(255, 225, 77)");
});

test("the board shows Reconnecting when the Hub is lost and keeps its tickets", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "new")]));
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-top-bar").getByText("Connected")).toBeVisible();
  stub.drop();
  await expect(page.getByTestId("kitchen-top-bar").getByText("Reconnecting")).toBeVisible();
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

const intents = (stub: Stub, kind: string) =>
  stub.sent.filter(
    (f) => f.kind === "intent.submit" && (f.intent as { kind: string }).kind === kind,
  ) as unknown as { intent: Record<string, unknown> }[];

test("a tap moves a ticket to the top of the next column at once and shows the Undo toast", async ({
  page,
}) => {
  const stub = await connectDisplay(
    page,
    board([
      ticket(12, "new", { rev: 3, seat: "PC 12" }),
      ticket(11, "preparing", { placed_at: HUB_NOW - 1 * MIN }),
    ]),
  );
  await page.goto("/kitchen/");
  await page.locator('[data-ticket="T12"]').click();

  // Sent with the ticket's rev and fresh ULIDs.
  await expect.poll(() => intents(stub, "kitchen.advance").length).toBe(1);
  const sent = intents(stub, "kitchen.advance")[0].intent;
  expect(sent.base_rev).toBe(3);
  expect(sent.payload).toEqual({ kind: "kitchen.advance", ticket_id: "T12", to: "preparing" });
  expect(sent.idempotency_key).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
  expect(sent.intent_id).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
  expect(sent.idempotency_key).not.toBe(sent.intent_id);

  // At once: first in Preparing, ahead of the newer ticket already there.
  await expect(column(page, "new").locator('[data-ticket="T12"]')).toHaveCount(0);
  await expect(column(page, "preparing").getByTestId("kitchen-ticket").first()).toHaveAttribute(
    "data-ticket",
    "T12",
  );
  const toast = page.getByTestId("undo-toast");
  await expect(toast.getByTestId("undo-message")).toHaveText("PC 12 · #K-12 moved to Preparing");
  await expect(toast.getByTestId("undo-seconds")).toHaveText("8 s left");
  await expect(toast.getByRole("button", { name: "Undo" })).toBeVisible();

  // The toast counts down and goes at 8 s: the advance is final.
  await page.clock.runFor(3_000);
  await expect(toast.getByTestId("undo-seconds")).toHaveText("5 s left");
  await page.clock.runFor(5_500);
  await expect(toast).toHaveCount(0);
});

test("Enter on a focused ticket advances it, and a ticket that moved keeps keyboard focus", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "ready")]));
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-board")).toBeVisible();
  await page.getByTestId("chime-toggle").focus();
  await page.keyboard.press("Tab");
  await expect(page.getByTestId("kitchen-ticket")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect.poll(() => intents(stub, "kitchen.advance").length).toBe(1);
  expect(intents(stub, "kitchen.advance")[0].intent.payload).toMatchObject({ to: "served" });
  await expect(column(page, "served").locator('[data-ticket="T1"]')).toBeFocused();
});

test("Space on a focused ticket advances it once", async ({ page }) => {
  const stub = await connectDisplay(page, board([ticket(1, "new")]));
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-board")).toBeVisible();
  await page.getByTestId("chime-toggle").focus();
  await page.keyboard.press("Tab");
  await expect(page.getByTestId("kitchen-ticket")).toBeFocused();
  await page.keyboard.press("Space");
  await expect.poll(() => intents(stub, "kitchen.advance").length).toBe(1);
  expect(intents(stub, "kitchen.advance")[0].intent.payload).toMatchObject({ to: "preparing" });
});

test("Undo within the window sends the advance's resulting rev and returns the ticket", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "new", { rev: 2 })]));
  await page.goto("/kitchen/");
  await page.locator('[data-ticket="T1"]').click();
  await expect(column(page, "preparing").locator('[data-ticket="T1"]')).toHaveCount(1);
  // The Hub pushes the board with the ticket at its new stage and rev.
  stub.push({
    kind: "kitchen.board",
    ...board([ticket(1, "preparing", { rev: 3, stage_at: HUB_NOW })]),
    now_ms: HUB_NOW + 1_000,
  });
  await page.getByRole("button", { name: "Undo" }).click();

  await expect.poll(() => intents(stub, "kitchen.advance.reverted").length).toBe(1);
  const sent = intents(stub, "kitchen.advance.reverted")[0].intent;
  expect(sent.base_rev).toBe(3);
  expect(sent.payload).toEqual({ kind: "kitchen.advance.reverted", ticket_id: "T1" });
  await expect(column(page, "new").locator('[data-ticket="T1"]')).toHaveCount(1);
  await expect(page.getByTestId("undo-toast")).toHaveCount(0);

  // The Hub's own board agrees.
  stub.push({
    kind: "kitchen.board",
    ...board([ticket(1, "new", { rev: 4 })]),
    now_ms: HUB_NOW + 2_000,
  });
  await expect(column(page, "new").locator('[data-ticket="T1"]')).toHaveCount(1);
});

test("advancing another ticket replaces the toast and the first advance stays", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "new"), ticket(2, "new")]));
  await page.goto("/kitchen/");
  await page.locator('[data-ticket="T1"]').click();
  await expect(page.getByTestId("undo-message")).toContainText("#K-1 moved to Preparing");
  await page.locator('[data-ticket="T2"]').click();
  await expect(page.getByTestId("undo-toast")).toHaveCount(1);
  await expect(page.getByTestId("undo-message")).toContainText("#K-2 moved to Preparing");
  await page.getByRole("button", { name: "Undo" }).click();
  await expect.poll(() => intents(stub, "kitchen.advance.reverted").length).toBe(1);
  expect(intents(stub, "kitchen.advance.reverted")[0].intent.payload).toMatchObject({
    ticket_id: "T2",
  });
  await expect(column(page, "preparing").locator('[data-ticket="T1"]')).toHaveCount(1);
});

test("a Served or cancelled ticket does nothing when tapped", async ({ page }) => {
  const stub = await connectDisplay(
    page,
    board([
      ticket(1, "served", { stage_at: HUB_NOW - MIN }),
      ticket(2, "cancelled", { stage_at: HUB_NOW }),
    ]),
  );
  await page.goto("/kitchen/");
  await page.locator('[data-ticket="T1"]').click();
  await page.locator('[data-ticket="T2"]').click();
  await page.locator('[data-ticket="T1"]').press("Enter");
  await expect(page.getByRole("button", { name: /Ticket/ })).toHaveCount(0);
  await expect(page.getByTestId("undo-toast")).toHaveCount(0);
  expect(stub.sent.filter((f) => f.kind === "intent.submit")).toHaveLength(0);
  await expect(column(page, "served").locator('[data-ticket="T1"]')).toHaveCount(1);
});

for (const code of ["STALE", "BAD_STAGE", "NOT_FOUND", "TIER_LOCKED"]) {
  test(`a ${code} answer snaps the ticket back to the Hub's board with no Undo`, async ({
    page,
  }) => {
    const stub = await connectDisplay(page, board([ticket(1, "new")]));
    stub.refuse("kitchen.advance", code);
    await page.goto("/kitchen/");
    await page.locator('[data-ticket="T1"]').click();
    const toast = page.getByTestId("undo-toast");
    await expect(toast.getByTestId("undo-notice")).toContainText("Already moved at the counter");
    await expect(toast.getByRole("button", { name: "Undo" })).toHaveCount(0);
    await expect(column(page, "new").locator('[data-ticket="T1"]')).toHaveCount(1);
    await expect(column(page, "preparing").locator('[data-ticket="T1"]')).toHaveCount(0);
    // It asked the Hub for its board again.
    expect(stub.sent.filter((f) => f.kind === "kitchen.board").length).toBeGreaterThanOrEqual(2);
    await page.clock.runFor(5_500);
    await expect(toast).toHaveCount(0);
  });
}

test("an Undo the Hub refuses as EXPIRED keeps the Hub's state and says it is too late", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "new")]));
  await page.goto("/kitchen/");
  await page.locator('[data-ticket="T1"]').click();
  stub.refuse("kitchen.advance.reverted", "EXPIRED");
  stub.push({
    kind: "kitchen.board",
    ...board([ticket(1, "preparing", { rev: 2, stage_at: HUB_NOW })]),
  });
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(page.getByTestId("undo-notice")).toContainText("Too late to undo");
  await expect(column(page, "preparing").locator('[data-ticket="T1"]')).toHaveCount(1);
});

test("a board push about another ticket does not undo a move that is still waiting", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "new"), ticket(2, "new")]));
  await page.goto("/kitchen/");
  await page.locator('[data-ticket="T1"]').click();
  stub.push({ kind: "kitchen.board", ...board([ticket(1, "new"), ticket(2, "preparing")]) });
  await expect(column(page, "preparing").locator('[data-ticket="T2"]')).toHaveCount(1);
  await expect(column(page, "preparing").locator('[data-ticket="T1"]')).toHaveCount(1);
});

test("the board with a toast has no accessibility violations", async ({ page }) => {
  await connectDisplay(page, board([ticket(2, "new"), ticket(1, "ready")]));
  await page.goto("/kitchen/");
  await page.locator('[data-ticket="T2"]').click();
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
});

// Story 7.9: the new-ticket chime, the NEW badge and the mute toggle. The browser's audio is a stub
// that records each play and, when `blocked`, refuses until the page has been tapped.
const MUTE_KEY = "arena.kitchen.chime-muted";

async function stubAudio(page: Page, blocked = false) {
  await page.addInitScript((blockedAtStart) => {
    const w = window as unknown as { __plays: number[]; __gesture: boolean };
    w.__plays = [];
    w.__gesture = !blockedAtStart;
    addEventListener("pointerdown", () => (w.__gesture = true), true);
    HTMLMediaElement.prototype.play = function (this: HTMLMediaElement) {
      if (!w.__gesture) return Promise.reject(new DOMException("blocked", "NotAllowedError"));
      w.__plays.push(this.volume);
      return Promise.resolve();
    };
    HTMLMediaElement.prototype.pause = () => {};
    if (blockedAtStart) {
      (navigator as unknown as { getAutoplayPolicy: () => string }).getAutoplayPolicy = () =>
        "disallowed";
    }
  }, blocked);
}

/** How many times the chime sounded (a silent unlock play has volume 0). */
const chimes = (page: Page) =>
  page.evaluate(
    () => (window as unknown as { __plays: number[] }).__plays.filter((v) => v === 1).length,
  );

const push = (stub: Stub, tickets: KitchenBoardTicket[]) =>
  stub.push({ kind: "kitchen.board", ...board(tickets) });

test("a new ticket arrives with the NEW badge, a danger edge, one chime and an announcement", async ({
  page,
}) => {
  await stubAudio(page);
  const stub = await connectDisplay(page, board([ticket(1, "new")]));
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-board")).toBeVisible();
  // Tickets already on the board when it loads are badged but do not chime.
  await expect(page.locator('[data-ticket="T1"]').getByTestId("ticket-new")).toHaveText("NEW");
  expect(await chimes(page)).toBe(0);

  push(stub, [ticket(2, "new", { placed_at: HUB_NOW }), ticket(1, "new")]);
  const fresh = page.locator('[data-ticket="T2"]');
  await expect(fresh.getByTestId("ticket-new")).toHaveText("NEW");
  await expect(fresh).toHaveAttribute("data-new", "");
  await expect(column(page, "new").getByTestId("kitchen-ticket").first()).toHaveAttribute(
    "data-ticket",
    "T2",
  );
  const edge = await fresh.evaluate((el) => getComputedStyle(el).borderLeftColor);
  const danger = await page.evaluate(() => {
    const probe = document.createElement("i");
    probe.style.color = "var(--danger)";
    document.body.append(probe);
    const colour = getComputedStyle(probe).color;
    probe.remove();
    return colour;
  });
  expect(edge).toBe(danger);
  await expect.poll(() => chimes(page)).toBe(1);
  await expect(page.getByTestId("board-announce")).toHaveText("New ticket K-2 for PC 02");
});

test("several tickets arriving together chime once and each gets its own badge", async ({
  page,
}) => {
  await stubAudio(page);
  const stub = await connectDisplay(page, board([]));
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-board")).toBeVisible();

  push(stub, [ticket(1, "new"), ticket(2, "new"), ticket(3, "new")]);
  await expect(page.getByTestId("ticket-new")).toHaveCount(3);
  // A second board in the same second shares the chime; one a few seconds later has its own.
  push(stub, [ticket(1, "new"), ticket(2, "new"), ticket(3, "new"), ticket(4, "new")]);
  await expect(page.getByTestId("ticket-new")).toHaveCount(4);
  expect(await chimes(page)).toBe(1);
  await page.clock.runFor(2_000);
  push(
    stub,
    [1, 2, 3, 4, 5].map((n) => ticket(n, "new")),
  );
  await expect(page.getByTestId("ticket-new")).toHaveCount(5);
  await expect.poll(() => chimes(page)).toBe(2);
});

test("advancing clears the badge for good: Undo back to New does not bring it back", async ({
  page,
}) => {
  await stubAudio(page);
  const stub = await connectDisplay(page, board([ticket(1, "new")]));
  await page.goto("/kitchen/");
  const badge = page.locator('[data-ticket="T1"]').getByTestId("ticket-new");
  await expect(badge).toBeVisible();

  await page.locator('[data-ticket="T1"]').click();
  await expect(badge).toHaveCount(0);
  await expect(page.locator('[data-ticket="T1"]')).not.toHaveAttribute("data-new", "");
  push(stub, [ticket(1, "preparing", { rev: 2, stage_at: HUB_NOW })]);
  await page.getByRole("button", { name: "Undo" }).click();
  push(stub, [ticket(1, "new", { rev: 3 })]);
  await expect(column(page, "new").locator('[data-ticket="T1"]')).toHaveCount(1);
  await expect(page.getByTestId("ticket-new")).toHaveCount(0);
  expect(await chimes(page)).toBe(0);
});

test("a ticket the Hub refuses to advance keeps its NEW badge", async ({ page }) => {
  await stubAudio(page);
  const stub = await connectDisplay(page, board([ticket(1, "new")]));
  stub.refuse("kitchen.advance", "STALE");
  await page.goto("/kitchen/");
  await page.locator('[data-ticket="T1"]').click();
  await expect(page.getByTestId("undo-notice")).toBeVisible();
  await expect(page.locator('[data-ticket="T1"]').getByTestId("ticket-new")).toBeVisible();
});

test("the mute toggle silences the chime, keeps the badges, and is kept for this tablet", async ({
  page,
}) => {
  await stubAudio(page);
  const stub = await connectDisplay(page, board([]));
  await page.goto("/kitchen/");
  const toggle = page.getByTestId("chime-toggle");
  expect((await toggle.boundingBox())?.height).toBeGreaterThanOrEqual(44);
  await expect(toggle).toHaveText("Chime on");

  await toggle.click();
  await expect(toggle).toHaveText("Chime off");
  expect(await page.evaluate((k) => localStorage.getItem(k), MUTE_KEY)).toBe("1");
  push(stub, [ticket(1, "new")]);
  await expect(page.getByTestId("ticket-new")).toHaveCount(1);
  expect(await chimes(page)).toBe(0);

  // Unmuted again: the next ticket chimes.
  await toggle.click();
  await expect(toggle).toHaveText("Chime on");
  expect(await page.evaluate((k) => localStorage.getItem(k), MUTE_KEY)).toBeNull();
  await page.clock.runFor(2_000);
  push(stub, [ticket(1, "new"), ticket(2, "new")]);
  await expect.poll(() => chimes(page)).toBe(1);
});

test("the mute choice survives a reload", async ({ page }) => {
  await stubAudio(page);
  await connectDisplay(page, board([]));
  await page.goto("/kitchen/");
  await page.getByTestId("chime-toggle").click();
  await page.reload();
  await expect(page.getByTestId("chime-toggle")).toHaveText("Chime off");
});

test("with browser storage unavailable the board starts unmuted", async ({ page }) => {
  await stubAudio(page);
  await connectDisplay(page, board([]));
  await page.addInitScript((key) => {
    const get = Storage.prototype.getItem;
    Storage.prototype.getItem = function (this: Storage, k: string) {
      if (k === key) throw new DOMException("denied", "SecurityError");
      return get.call(this, k);
    };
  }, MUTE_KEY);
  await page.goto("/kitchen/");
  await expect(page.getByTestId("chime-toggle")).toHaveText("Chime on");
});

test("a browser that blocks audio shows the tap bar until the first tap, and badges work either way", async ({
  page,
}) => {
  await stubAudio(page, true);
  const stub = await connectDisplay(page, board([]));
  await page.goto("/kitchen/");
  const bar = page.getByTestId("chime-unlock");
  await expect(bar).toHaveText("Tap to turn on the chime");

  push(stub, [ticket(1, "new")]);
  await expect(page.getByTestId("ticket-new")).toHaveCount(1);
  expect(await chimes(page)).toBe(0);

  await page.getByTestId("board-branch").click();
  await expect(bar).toHaveCount(0);
  await page.clock.runFor(2_000);
  push(stub, [ticket(1, "new"), ticket(2, "new")]);
  await expect.poll(() => chimes(page)).toBe(1);
  await expect(page.getByTestId("ticket-new")).toHaveCount(2);
});

test("the tap bar does not show when the browser allows sound", async ({ page }) => {
  await stubAudio(page);
  await connectDisplay(page, board([]));
  await page.goto("/kitchen/");
  await expect(page.getByTestId("kitchen-board")).toBeVisible();
  await expect(page.getByTestId("chime-unlock")).toHaveCount(0);
});

test("the board with NEW badges, the toggle and the tap bar has no accessibility violations", async ({
  page,
}) => {
  await stubAudio(page, true);
  await connectDisplay(page, board([ticket(2, "new"), ticket(1, "ready")]));
  await page.goto("/kitchen/");
  await expect(page.getByTestId("chime-unlock")).toBeVisible();
  await expect(page.getByTestId("ticket-new")).toHaveCount(1);
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
  await page.getByTestId("chime-toggle").click();
  await expect(page.getByTestId("chime-toggle")).toHaveText("Chime off");
  const muted = await new AxeBuilder({ page }).analyze();
  expect(muted.violations).toEqual([]);
});

// Work through LAN loss (Story 7.10).

type Stored = {
  key: string;
  ticketId: string;
  to: string;
  baseRev: number;
  issuedAt: number;
  expiresAt: number;
};

/** The advances waiting in the display's IndexedDB queue, oldest first. */
const stored = (page: Page) =>
  page.evaluate(
    () =>
      new Promise<Stored[]>((resolve) => {
        const open = indexedDB.open("arena.kitchen", 1);
        open.onsuccess = () => {
          const all = open.result.transaction("queue").objectStore("queue").getAll();
          all.onsuccess = () =>
            resolve((all.result as Stored[]).sort((a, b) => a.issuedAt - b.issuedAt));
        };
        open.onerror = () => resolve([]);
      }),
  );

/** Runs the page's clock a second at a time until `done` holds (the reconnect pause is jittered). */
async function advanceUntil(page: Page, done: () => Promise<boolean> | boolean, seconds = 120) {
  for (let i = 0; i < seconds; i++) {
    if (await done()) return;
    await page.clock.runFor(1_000);
  }
  expect(await done()).toBe(true);
}

const bar = (page: Page) => page.getByTestId("kitchen-top-bar");
const showsConnected = async (page: Page) => (await bar(page).getByText("Connected").count()) > 0;
const advances = (stub: Stub) =>
  intents(stub, "kitchen.advance").map((f) => f.intent as Record<string, unknown>);
const settled = async (page: Page) =>
  (await showsConnected(page)) && (await stored(page)).length === 0;

async function loseHub(page: Page, stub: Stub) {
  stub.reachable(false);
  stub.drop();
  await expect(bar(page).getByText("Reconnecting")).toBeVisible();
}

test("with the Hub lost a tap moves the ticket at once, is queued and an Undo takes it out", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "new"), ticket(2, "new")]));
  await page.goto("/kitchen/");
  await expect(bar(page).getByText("Connected")).toBeVisible();
  await loseHub(page, stub);

  await page.locator('[data-ticket="T1"]').click();
  await expect(column(page, "preparing").locator('[data-ticket="T1"]')).toHaveCount(1);
  await expect(page.getByTestId("undo-toast")).toBeVisible();
  await expect.poll(() => stored(page)).toHaveLength(1);
  const [first] = await stored(page);
  expect(first).toMatchObject({ ticketId: "T1", to: "preparing", baseRev: 1 });
  expect(first.key).toMatch(/^[0-9A-Z]{26}$/);
  expect(first.expiresAt - first.issuedAt).toBe(24 * 60 * MIN);

  // A second tap on the same ticket chains: it is based on the rev the first will leave.
  await page.locator('[data-ticket="T1"]').click();
  await expect(column(page, "ready").locator('[data-ticket="T1"]')).toHaveCount(1);
  await expect.poll(() => stored(page)).toHaveLength(2);
  expect((await stored(page))[1]).toMatchObject({ ticketId: "T1", to: "ready", baseRev: 2 });

  // No marker on any ticket, and the page has nothing to flag.
  await expect(page.getByTestId("kitchen-ticket").first()).not.toContainText(/offline|queued/i);
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);

  // Undo of a move that was never sent removes it from the queue.
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(column(page, "preparing").locator('[data-ticket="T1"]')).toHaveCount(1);
  await expect.poll(async () => (await stored(page)).map((e) => e.to)).toEqual(["preparing"]);
  expect(advances(stub)).toHaveLength(0);

  // Another ticket's move and its Undo leave the queue as it was.
  await page.locator('[data-ticket="T2"]').click();
  await expect.poll(async () => (await stored(page)).length).toBe(2);
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(column(page, "new").locator('[data-ticket="T2"]')).toHaveCount(1);
  await expect.poll(async () => (await stored(page)).map((e) => e.ticketId)).toEqual(["T1"]);
});

test("the Hub's return replays the queue in order once each, redraws the board and only then says Connected", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "new"), ticket(2, "new")]));
  await page.goto("/kitchen/");
  await expect(bar(page).getByText("Connected")).toBeVisible();
  await loseHub(page, stub);

  await page.locator('[data-ticket="T1"]').click();
  await page.locator('[data-ticket="T2"]').click();
  await page.locator('[data-ticket="T1"]').click();
  await expect.poll(() => stored(page)).toHaveLength(3);
  const keys = (await stored(page)).map((e) => e.key);
  expect(new Set(keys).size).toBe(3);

  // An order placed at the counter meanwhile is not on the display yet.
  stub.setBoard(
    board([
      ticket(1, "ready", { rev: 3 }),
      ticket(2, "preparing", { rev: 2 }),
      ticket(3, "new", { placed_at: HUB_NOW }),
    ]),
  );
  await page.clock.runFor(5_000);
  await expect(page.locator('[data-ticket="T3"]')).toHaveCount(0);

  // The Hub is back but does not answer yet: the badge stays on Reconnecting.
  stub.silent(true);
  stub.reachable(true);
  await advanceUntil(page, () => advances(stub).length === 1);
  await expect(bar(page).getByText("Reconnecting")).toBeVisible();
  expect(advances(stub)[0].idempotency_key).toBe(keys[0]);

  // No reply in 5 s drops the link; the same key is sent again, never a new one.
  stub.silent(false);
  await advanceUntil(page, () => settled(page));
  const sent = advances(stub);
  expect(sent.map((i) => i.idempotency_key).filter((k, at, all) => all.indexOf(k) === at)).toEqual(
    keys,
  );
  expect(sent.map((i) => [i.idempotency_key, i.base_rev]).slice(-3)).toEqual([
    [keys[0], 1],
    [keys[1], 1],
    [keys[2], 2],
  ]);
  expect(stub.received.get(keys[0])).toBe(2);
  expect(stub.received.get(keys[1])).toBe(1);
  expect(stub.received.get(keys[2])).toBe(1);

  // The board is the Hub's: T3 arrived, with the NEW badge.
  await expect(column(page, "ready").locator('[data-ticket="T1"]')).toHaveCount(1);
  await expect(column(page, "preparing").locator('[data-ticket="T2"]')).toHaveCount(1);
  await expect(column(page, "new").locator('[data-ticket="T3"]')).toHaveCount(1);
  await expect(page.getByTestId("ticket-new")).toHaveCount(1);
});

test("a queued advance the Hub refuses is dropped without a word and the ticket shows the Hub's state", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "new"), ticket(2, "new")]));
  await page.goto("/kitchen/");
  await expect(bar(page).getByText("Connected")).toBeVisible();
  await loseHub(page, stub);
  await page.locator('[data-ticket="T1"]').click();
  await page.locator('[data-ticket="T2"]').click();
  await expect.poll(() => stored(page)).toHaveLength(2);

  // The counter moved T1 to Ready while the display was away.
  stub.refuse("kitchen.advance", "STALE", "T1");
  stub.setBoard(board([ticket(1, "ready", { rev: 3 }), ticket(2, "preparing", { rev: 2 })]));
  stub.reachable(true);
  await advanceUntil(page, () => settled(page));

  await expect(column(page, "ready").locator('[data-ticket="T1"]')).toHaveCount(1);
  await expect(column(page, "preparing").locator('[data-ticket="T2"]')).toHaveCount(1);
  await expect(page.getByTestId("undo-notice")).toHaveCount(0);
});

test("after 10 minutes without the Hub 20 advances apply once each, a cancelled ticket shows Cancelled and elapsed time kept counting", async ({
  page,
}) => {
  const tickets = Array.from({ length: 21 }, (_, i) => ticket(i + 1, "new"));
  const stub = await connectDisplay(page, board(tickets));
  await page.goto("/kitchen/");
  await expect(bar(page).getByText("Connected")).toBeVisible();
  await loseHub(page, stub);

  for (let n = 1; n <= 20; n++) await page.locator(`[data-ticket="T${n}"]`).click();
  await expect.poll(async () => (await stored(page)).length).toBe(20);
  const keys = (await stored(page)).map((e) => e.key);

  // Ten minutes pass; the untouched ticket was placed 5 minutes before the last board.
  await page.clock.runFor(10 * MIN);
  await expect(bar(page).getByText("Reconnecting")).toBeVisible();
  await expect(page.locator('[data-ticket="T21"]').getByTestId("ticket-elapsed")).toHaveText(
    /^15:0\d$/,
  );

  // Meanwhile the counter cancelled T7: the Hub refuses that advance.
  const later = HUB_NOW + 10 * MIN;
  stub.refuse("kitchen.advance", "BAD_STAGE", "T7");
  stub.setBoard({
    branch_name: "Arena Indiranagar",
    now_ms: later,
    tickets: tickets.map((t) =>
      t.id === "T7"
        ? { ...t, stage: "cancelled", stage_at: later - 1_000, rev: 2 }
        : t.id === "T21"
          ? t
          : { ...t, stage: "preparing", stage_at: later - 5_000, rev: 2 },
    ),
  });
  stub.reachable(true);
  await advanceUntil(page, () => settled(page));

  const sent = advances(stub).map((i) => i.idempotency_key as string);
  expect(sent).toEqual(keys);
  for (const key of keys) expect(stub.received.get(key)).toBe(1);
  await expect(column(page, "preparing").getByTestId("kitchen-ticket")).toHaveCount(19);
  await expect(column(page, "new").locator('[data-ticket="T7"]')).toContainText("Cancelled");
  await expect(column(page, "new").locator('[data-ticket="T21"]')).toHaveCount(1);
});

test("a reload during LAN loss keeps the queue, and it replays before new taps are sent", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "new"), ticket(2, "new")]));
  await page.goto("/kitchen/");
  await expect(bar(page).getByText("Connected")).toBeVisible();
  await loseHub(page, stub);
  await page.locator('[data-ticket="T1"]').click();
  await expect.poll(() => stored(page)).toHaveLength(1);
  const [kept] = await stored(page);

  // The board cannot load while the Hub is out of reach, but the queue stays.
  await page.reload();
  await expect(page.getByTestId("kitchen-board")).toHaveCount(0);
  await page.clock.runFor(5_000);
  await expect(page.getByTestId("kitchen-board")).toHaveCount(0);
  expect(await stored(page)).toHaveLength(1);

  // The Hub returns but is slow to answer: the board is up, the stored move goes first, and a
  // new tap waits behind it.
  stub.silent(true);
  stub.reachable(true);
  await advanceUntil(page, () => advances(stub).length === 1);
  await expect(page.getByTestId("kitchen-board")).toBeVisible();
  expect(advances(stub)[0].idempotency_key).toBe(kept.key);
  await page.locator('[data-ticket="T2"]').click();
  expect(advances(stub)).toHaveLength(1);

  stub.silent(false);
  await advanceUntil(page, () => settled(page));
  const order = advances(stub)
    .map((i) => i.idempotency_key as string)
    .filter((k, at, all) => all.indexOf(k) === at);
  expect(order).toHaveLength(2);
  expect(order[0]).toBe(kept.key);
  expect(stub.received.get(order[1])).toBe(1);
});

test("a tap the Hub does not answer within 5 s counts as a lost link and is kept for the replay", async ({
  page,
}) => {
  const stub = await connectDisplay(page, board([ticket(1, "new")]));
  await page.goto("/kitchen/");
  await expect(bar(page).getByText("Connected")).toBeVisible();
  stub.silent(true);
  await page.locator('[data-ticket="T1"]').click();
  await expect(column(page, "preparing").locator('[data-ticket="T1"]')).toHaveCount(1);
  await page.clock.runFor(4_000);
  await expect(bar(page).getByText("Connected")).toBeVisible();
  await page.clock.runFor(1_500);
  await expect(bar(page).getByText("Reconnecting")).toBeVisible();
  await expect(column(page, "preparing").locator('[data-ticket="T1"]')).toHaveCount(1);
  await expect.poll(() => stored(page)).toHaveLength(1);

  stub.silent(false);
  await advanceUntil(page, () => settled(page));
  const key = advances(stub)[0].idempotency_key as string;
  expect(stub.received.get(key)).toBe(2);
});

test("a stored move older than 24 hours is dropped instead of sent", async ({ page }) => {
  const stub = await connectDisplay(page, board([ticket(1, "new")]));
  await page.goto("/kitchen/");
  await expect(bar(page).getByText("Connected")).toBeVisible();
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        const open = indexedDB.open("arena.kitchen", 1);
        open.onsuccess = () => {
          const tx = open.result.transaction("queue", "readwrite");
          const old = Date.now() - 25 * 60 * 60_000;
          tx.objectStore("queue").put({
            key: "01JOLDADVANCE0000000000000",
            ticketId: "T1",
            to: "preparing",
            baseRev: 1,
            issuedAt: old,
            expiresAt: old + 24 * 60 * 60_000,
          });
          tx.oncomplete = () => resolve();
        };
      }),
  );
  await page.reload();
  await expect(bar(page).getByText("Connected")).toBeVisible();
  await expect.poll(() => stored(page)).toHaveLength(0);
  expect(advances(stub)).toHaveLength(0);
  await expect(column(page, "new").locator('[data-ticket="T1"]')).toHaveCount(1);
});
