import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import names from "../src-tauri/ipc-names.json" with { type: "json" };
import {
  add,
  markRead,
  MAX_MESSAGES,
  noteText,
  orderCopy,
  timeOf,
  unread,
  type Note,
} from "../src/lib/messages";

// Story 6.16: the messages bell in the shell's top bar. The Tauri shell is stubbed in the page:
// the test emits its `staff-message`, `staff-messages-cleared` and `pc-screen` events as
// `@tauri-apps/api/event` delivers them, and `__calls` records what the page asked of it.

type Held = { id: number; text: string; at: number; read: boolean; fresh: boolean };

async function openWithShell(page: Page, held: Held[] = []) {
  await page.addInitScript(
    ([commands, initial]: [typeof names.commands, Held[]]) => {
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
            case commands.staffMessages:
              return initial;
            case commands.messagesRead:
              (w.__calls as unknown[]).push(["messages_read"]);
              return { ok: true, body: {} };
            case commands.volumeGet:
              return { ok: true, body: { percent: 50 } };
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
    [names.commands, held] as [typeof names.commands, Held[]],
  );
  await page.goto("/");
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.pcScreen,
  );
  await session(page);
}

const emitTo = (page: Page, event: string, payload: unknown) =>
  page.evaluate(
    ([e, p]) =>
      (window as unknown as { __shell: { emit: (e: string, p: unknown) => void } }).__shell.emit(
        e as string,
        p,
      ),
    [event, payload] as const,
  );

const session = async (page: Page) => {
  await emitTo(page, "pc-screen", {
    locked: false,
    reason: null,
    session: { device_name: "PC 7", elapsed_ms: 0, username: "asha", remaining_ms: 3_600_000 },
  });
  await expect(page.getByTestId("shell")).toBeVisible();
  await page.waitForFunction(
    (event) =>
      (window as unknown as { __shell: { listening: (e: string) => boolean } }).__shell.listening(
        event,
      ),
    names.events.staffMessage,
  );
};

const message = (id: number, text: string, at: number, over: Partial<Held> = {}): Held => ({
  id,
  text,
  at,
  read: false,
  fresh: true,
  ...over,
});

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __calls: unknown[] }).__calls);

const AT = Date.UTC(2026, 9, 29, 8, 30);

test("the bell sits left of Call staff and has no badge while nothing is unread", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await openWithShell(page);
  const bell = page.getByTestId("messages-bell");
  const call = page.getByTestId("call-staff-button");
  expect((await bell.boundingBox())!.x).toBeLessThan((await call.boundingBox())!.x);
  expect((await bell.boundingBox())!.height).toBeGreaterThanOrEqual(44);
  await expect(bell).toHaveAccessibleName(en["player.messages.label"]);
  await expect(page.getByTestId("messages-bell-dot")).toHaveCount(0);
});

test("an unread message adds an info dot and a count that is part of the accessible name", async ({
  page,
}) => {
  await openWithShell(page);
  const bell = page.getByTestId("messages-bell");
  await emitTo(page, names.events.staffMessage, message(1, "Closing at 11", AT));
  await expect(bell).toHaveAccessibleName(en["player.messages.label_unread_one"]);
  await expect(page.getByTestId("messages-bell-count")).toHaveText("1");
  const dot = await page
    .getByTestId("messages-bell-dot")
    .locator(".dot")
    .evaluate((el) => getComputedStyle(el).backgroundColor);
  const info = await page.evaluate(() => {
    const probe = document.createElement("i");
    probe.style.color = "var(--info)";
    document.body.append(probe);
    const colour = getComputedStyle(probe).color;
    probe.remove();
    return colour;
  });
  expect(dot).toBe(info);
  await emitTo(page, names.events.staffMessage, message(2, "Your order is ready", AT + 60_000));
  await expect(bell).toHaveAccessibleName(
    en["player.messages.label_unread"].replace("{count}", "2"),
  );
  await expect(page.getByTestId("messages-bell-count")).toHaveText("2");
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
});

test("opening the bell lists the messages newest first with their time and marks them read", async ({
  page,
}) => {
  await openWithShell(page);
  const bell = page.getByTestId("messages-bell");
  await emitTo(page, names.events.staffMessage, message(1, "Closing at 11", AT));
  await emitTo(page, names.events.staffMessage, message(2, "Your order is ready", AT + 120_000));
  await expect(page.getByTestId("messages-panel")).toHaveCount(0);
  await bell.click();
  const panel = page.getByTestId("messages-panel");
  await expect(panel).toBeVisible();
  const items = panel.getByTestId("message-item");
  await expect(items).toHaveCount(2);
  await expect(items.nth(0).getByTestId("message-text")).toHaveText("Your order is ready");
  await expect(items.nth(1).getByTestId("message-text")).toHaveText("Closing at 11");
  const expected = await page.evaluate(
    (at) =>
      new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" }).format(
        new Date(at),
      ),
    AT + 120_000,
  );
  await expect(items.nth(0).locator("time")).toHaveText(expected);
  await expect(page.getByTestId("messages-bell-dot")).toHaveCount(0);
  await expect(bell).toHaveAccessibleName(en["player.messages.label"]);
  await expect.poll(() => calls(page)).toContainEqual(["messages_read"]);
  expect(
    (await new AxeBuilder({ page }).analyze()).violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
});

test("a message that comes in while the list is open is read at once", async ({ page }) => {
  await openWithShell(page);
  await page.getByTestId("messages-bell").click();
  await expect(page.getByTestId("messages-empty")).toHaveText(en["player.messages.empty"]);
  await emitTo(page, names.events.staffMessage, message(1, "Hello", AT));
  await expect(page.getByTestId("message-item")).toHaveCount(1);
  await expect(page.getByTestId("messages-bell-dot")).toHaveCount(0);
});

test("the bell works by keyboard: Enter and Space open it, Esc closes it and returns focus", async ({
  page,
}) => {
  await openWithShell(page);
  await emitTo(page, names.events.staffMessage, message(1, "Closing at 11", AT));
  const bell = page.getByTestId("messages-bell");
  await bell.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("messages-panel")).toBeVisible();
  await expect(bell).toHaveAttribute("aria-expanded", "true");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("messages-panel")).toHaveCount(0);
  await expect(bell).toBeFocused();
  await page.keyboard.press("Space");
  await expect(page.getByTestId("messages-panel")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(bell).toBeFocused();
});

test("a click away closes the list", async ({ page }) => {
  await openWithShell(page);
  await page.getByTestId("messages-bell").click();
  await expect(page.getByTestId("messages-panel")).toBeVisible();
  await page.getByTestId("session-device").click();
  await expect(page.getByTestId("messages-panel")).toHaveCount(0);
});

test("messages the shell already held show when the page opens, and a replay is not added twice", async ({
  page,
}) => {
  await openWithShell(page, [
    message(2, "Second", AT + 1000, { fresh: false }),
    message(1, "First", AT, { read: true, fresh: false }),
  ]);
  const bell = page.getByTestId("messages-bell");
  await expect(bell).toHaveAccessibleName(en["player.messages.label_unread_one"]);
  await emitTo(page, names.events.staffMessage, message(2, "Second", AT + 1000, { fresh: false }));
  await bell.click();
  await expect(page.getByTestId("message-item")).toHaveCount(2);
});

test("the list is empty for the next player once the session has ended", async ({ page }) => {
  await openWithShell(page);
  await emitTo(page, names.events.staffMessage, message(1, "Closing at 11", AT));
  await expect(page.getByTestId("messages-bell-count")).toHaveText("1");
  await emitTo(page, "pc-screen", { locked: true, reason: "pay_at_counter", session: null });
  await expect(page.getByTestId("shell")).toHaveCount(0);
  await emitTo(page, "pc-screen", {
    locked: false,
    reason: null,
    session: { device_name: "PC 7", elapsed_ms: 0, username: "ravi", remaining_ms: 3_600_000 },
  });
  await expect(page.getByTestId("shell")).toBeVisible();
  await expect(page.getByTestId("messages-bell-dot")).toHaveCount(0);
  await page.getByTestId("messages-bell").click();
  await expect(page.getByTestId("messages-empty")).toBeVisible();
});

test("the shell dropping its messages empties the list", async ({ page }) => {
  await openWithShell(page);
  await emitTo(page, names.events.staffMessage, message(1, "Closing at 11", AT));
  await expect(page.getByTestId("messages-bell-count")).toHaveText("1");
  await emitTo(page, names.events.staffMessagesCleared, null);
  await expect(page.getByTestId("messages-bell-dot")).toHaveCount(0);
});

test("the bell has no backdrop blur and the open list is solid", async ({ page }) => {
  await openWithShell(page);
  await page.getByTestId("messages-bell").click();
  const paint = await page.getByTestId("messages-panel").evaluate((el) => {
    const s = getComputedStyle(el);
    return { background: s.backgroundColor, filter: s.backdropFilter };
  });
  expect(paint.background).not.toBe("rgba(0, 0, 0, 0)");
  expect(paint.filter === "none" || paint.filter === "").toBe(true);
});

const note = (id: number, read = false): Note => ({ id, text: `m${id}`, at: id, read });

test("the list rules: newest first, no duplicates, at most 50, unread counted, all marked read", () => {
  let list: Note[] = [];
  list = add(list, note(1));
  list = add(list, note(3));
  list = add(list, note(2, true));
  expect(list.map((n) => n.id)).toEqual([3, 2, 1]);
  expect(add(list, note(2)).length).toBe(3);
  expect(unread(list)).toBe(2);
  expect(unread(markRead(list))).toBe(0);
  let many: Note[] = [];
  for (let i = 1; i <= MAX_MESSAGES + 5; i++) many = add(many, note(i));
  expect(many.length).toBe(MAX_MESSAGES);
  expect(many[0].id).toBe(MAX_MESSAGES + 5);
  expect(timeOf(Date.UTC(2026, 9, 29, 8, 30))).toMatch(/\d/);
});

// Story 6.33: order notes are kept in the bell, worded from the catalogue.
test("order notes are listed in the bell with the order copy", async ({ page }) => {
  await openWithShell(page);
  const coded = (
    id: number,
    code: string,
    count = 0,
    text = "",
  ): Held & Record<string, unknown> => ({
    ...message(id, text, AT + id * 1000),
    code,
    count,
  });
  await emitTo(page, names.events.staffMessage, coded(1, "order.placed", 3));
  await emitTo(page, names.events.staffMessage, coded(2, "order.ready_counter"));
  await emitTo(page, names.events.staffMessage, coded(3, "order.cancelled", 0, "Burger"));
  await page.getByTestId("messages-bell").click();
  await expect(page.getByTestId("message-text")).toHaveText([
    "Burger was cancelled — see the counter.",
    en["player.order.ready_counter"],
    "Ordered: 3 items",
  ]);
});

test("the order copy rules word each code and leave staff text alone", () => {
  expect(orderCopy({ text: "Hi" })).toBeNull();
  expect(orderCopy({ text: "", code: "order.placed", count: 1 })?.key).toBe(
    "player.order.placed_one",
  );
  expect(orderCopy({ text: "", code: "order.placed", count: 4 })).toEqual({
    key: "player.order.placed",
    params: { count: 4 },
  });
  expect(orderCopy({ text: "Cola", code: "order.cancelled" })?.params).toEqual({ item: "Cola" });
  expect(noteText({ text: "Closing at 11" }, (k) => k)).toBe("Closing at 11");
  expect(noteText({ text: "", code: "order.ready_seat" }, (k) => k)).toBe(
    "player.order.ready_seat",
  );
});
