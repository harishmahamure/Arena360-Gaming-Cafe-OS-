import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };

// InlinePin and ReasonPicker on the gallery: keyboard entry, the keypad, rejection, Esc, the
// cell-count live region, and the stubbed reason-requiring flow (device.revoke). The demo PIN
// is 2222.

const THEMES = ["dark", "hc", "light"] as const;

async function open(page: Page) {
  await page.goto("/");
  await expect(page.locator("h1")).toBeVisible();
}

const section = (page: Page, theme: string) => page.locator(`[data-theme="${theme}"]`);
const sample = (page: Page, theme = "dark") => section(page, theme).getByTestId("pin-sample");
const flow = (page: Page, theme = "dark") => section(page, theme).getByTestId("reason-flow");

/** Opens the reason-requiring flow and picks `reason` (the picker comes before the PIN). */
async function revokeWithReason(page: Page, theme: string, reason: string) {
  const f = flow(page, theme);
  await f.getByRole("button", { name: en["gallery.approval.revoke"] }).click();
  await f.getByLabel(reason).check();
  await f.getByRole("button", { name: en["ui.reason.confirm"] }).click();
  await expect(f.getByLabel(en["ui.pin.label"])).toBeFocused();
  return f;
}

for (const theme of THEMES) {
  test(`PIN entry by keyboard: cells, count, rejection and approval (${theme})`, async ({
    page,
  }) => {
    await open(page);
    const pin = sample(page, theme);
    const input = pin.getByLabel(en["ui.pin.label"]);
    await expect(input).toHaveAttribute("inputmode", "numeric");
    await expect(input).toHaveAttribute("autocomplete", "off");
    await input.focus();
    await page.keyboard.type("12");
    // PINs of 4–6 digits: six cells.
    await expect(pin.locator(".cell")).toHaveCount(6);
    await expect(pin.locator(".cell.filled")).toHaveCount(2);
    const live = pin.locator('[aria-live="polite"]');
    await expect(live).toHaveText(
      en["ui.pin.count"].replace("{count}", "2").replace("{length}", "6"),
    );
    // Letters are ignored; Backspace removes the last digit.
    await page.keyboard.type("a");
    await expect(pin.locator(".cell.filled")).toHaveCount(2);
    await page.keyboard.press("Backspace");
    await expect(pin.locator(".cell.filled")).toHaveCount(1);
    // Enter before the fourth digit asks for the rest.
    await page.keyboard.press("Enter");
    await expect(pin).toContainText(en["ui.pin.incomplete"].replace("{length}", "4"));
    await page.keyboard.type("999");
    await page.keyboard.press("Enter");
    // Rejected: cells and the input clear at once, and the message shows inline.
    await expect(pin).toContainText(en["ui.pin.rejected"]);
    await expect(pin.locator(".cell.filled")).toHaveCount(0);
    await expect(input).toHaveValue("");
    await expect(input).toBeFocused();
    await expect(input).toHaveAttribute("aria-invalid", "true");
    // The sixth digit submits on its own.
    await page.keyboard.type("123456");
    await expect(pin).toContainText(en["ui.pin.rejected"]);
    await expect(pin.locator(".cell.filled")).toHaveCount(0);
    await page.keyboard.type("2222");
    // The digits never reach any text on the page.
    await expect(pin).not.toContainText("2222");
    await page.keyboard.press("Enter");
    await expect(pin.getByRole("status")).toHaveText(en["gallery.approval.approved"]);
    await expect(input).toHaveValue("");
  });
}

test("the keypad types, deletes and submits", async ({ page }) => {
  await open(page);
  const pin = sample(page);
  for (const d of ["2", "2", "2", "9", "1"]) {
    await pin.getByRole("button", { name: d, exact: true }).click();
  }
  await expect(pin.locator(".cell.filled")).toHaveCount(5);
  await pin.getByRole("button", { name: en["ui.pin.key_delete"] }).click();
  await pin.getByRole("button", { name: en["ui.pin.key_delete"] }).click();
  await expect(pin.locator(".cell.filled")).toHaveCount(3);
  await pin.getByRole("button", { name: "2", exact: true }).click();
  // Digits typed while a key has focus count too.
  await pin.getByRole("button", { name: en["ui.pin.key_delete"] }).focus();
  await page.keyboard.press("Backspace");
  await page.keyboard.press("2");
  await expect(pin.locator(".cell.filled")).toHaveCount(4);
  await pin.getByRole("button", { name: en["ui.pin.key_submit"] }).click();
  await expect(pin.getByRole("status")).toHaveText(en["gallery.approval.approved"]);
  // Every key is a 44px target.
  for (const key of await pin.locator(".key").all()) {
    const box = await key.boundingBox();
    expect(box!.width).toBeGreaterThanOrEqual(44);
    expect(box!.height).toBeGreaterThanOrEqual(44);
  }
});

test("Esc clears the PIN and returns focus to the control that opened it", async ({ page }) => {
  await open(page);
  const f = await revokeWithReason(page, "dark", en["reason.lost"]);
  const revoke = f.getByRole("button", { name: en["gallery.approval.revoke"] });
  await page.keyboard.type("12");
  await page.keyboard.press("Escape");
  await expect(f.locator('[data-component="inline-pin"]')).toHaveCount(0);
  await expect(revoke).toBeFocused();
  await expect(f.getByRole("status")).toHaveText(en["gallery.approval.cancelled"]);
  // Starting again asks for the reason again, then an empty PIN.
  await page.keyboard.press("Enter");
  await expect(f.locator('[data-component="reason-picker"]')).toBeVisible();
});

test("a reason-requiring intent: the reason picker, then the PIN, then applied with the reason", async ({
  page,
}) => {
  await open(page);
  const f = flow(page);
  const revoke = f.getByRole("button", { name: en["gallery.approval.revoke"] });
  await revoke.focus();
  await page.keyboard.press("Enter");
  // device.revoke needs a reason (the generated reasons table): the picker comes first.
  const picker = f.locator('[data-component="reason-picker"]');
  await expect(picker).toBeVisible();
  await expect(f.locator('[data-component="inline-pin"]')).toHaveCount(0);
  await expect(picker.getByRole("group", { name: en["ui.reason.legend"] })).toBeVisible();
  for (const code of ["lost", "stolen", "replaced", "other"] as const) {
    await expect(picker.getByLabel(en[`reason.${code}`])).toBeVisible();
  }
  const first = picker.getByLabel(en["reason.lost"]);
  await expect(first).toBeFocused();
  // Confirm with nothing chosen: the field error, and focus stays on the options.
  await page.keyboard.press("Tab");
  await page.keyboard.press("Enter");
  await expect(picker).toContainText(en["ui.reason.required"]);
  await expect(first).toBeFocused();
  // Keyboard only: arrow to Stolen, Enter confirms; the PIN takes its place.
  await page.keyboard.press("ArrowDown");
  await expect(picker.getByLabel(en["reason.stolen"])).toBeChecked();
  await page.keyboard.press("Enter");
  await expect(picker).toHaveCount(0);
  await expect(f.getByLabel(en["ui.pin.label"])).toBeFocused();
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(f.getByRole("status")).toHaveText(
    en["gallery.approval.revoked"].replace("{reason}", en["reason.stolen"]),
  );
  await expect(f.locator('[data-component="inline-pin"]')).toHaveCount(0);
  await expect(revoke).toBeFocused();
});

test("the reason picker focuses the chosen option and shows a refusal inline", async ({ page }) => {
  await open(page);
  const sampleCell = section(page, "dark").locator('.cell:has(> [data-component="reason-picker"])');
  const picker = sampleCell.locator('[data-component="reason-picker"]');
  await picker.getByLabel(en["reason.replaced"]).check();
  await picker.getByRole("button", { name: en["ui.reason.confirm"] }).click();
  // The gallery's static sample answers a refusal: shown inline, focus on the chosen option.
  await expect(picker).toContainText(en["ui.pin.unreachable"]);
  await expect(picker.getByLabel(en["reason.replaced"])).toBeFocused();
});

test("Esc in the reason picker cancels and returns focus", async ({ page }) => {
  await open(page);
  const f = flow(page, "hc");
  const revoke = f.getByRole("button", { name: en["gallery.approval.revoke"] });
  await revoke.click();
  await expect(f.locator('[data-component="reason-picker"]')).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(f.locator('[data-component="reason-picker"]')).toHaveCount(0);
  await expect(revoke).toBeFocused();
});

test("axe: the open PIN and reason picker have no serious or critical violations", async ({
  page,
}) => {
  await open(page);
  for (const theme of THEMES) {
    await flow(page, theme).getByRole("button", { name: en["gallery.approval.revoke"] }).click();
  }
  const picker = await new AxeBuilder({ page })
    .include('[data-component="reason-picker"]')
    .analyze();
  expect(
    picker.violations
      .filter((v) => v.impact === "serious" || v.impact === "critical")
      .map((v) => v.id),
  ).toEqual([]);
  for (const theme of THEMES) {
    const f = flow(page, theme);
    await f.getByLabel(en["reason.other"]).check();
    await f.getByRole("button", { name: en["ui.reason.confirm"] }).click();
    await expect(f.locator('[data-component="inline-pin"]')).toBeVisible();
  }
  const results = await new AxeBuilder({ page }).include('[data-component="inline-pin"]').analyze();
  const blocking = results.violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );
  expect(blocking.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" "))}`)).toEqual([]);
});
