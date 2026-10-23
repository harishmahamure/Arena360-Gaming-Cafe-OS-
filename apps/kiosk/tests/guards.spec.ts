import { expect, test } from "@playwright/test";

// The frontend lockdown guards (Story 1.17): no context menu, blocked browser accelerator keys,
// and a neutralised print. Defense in depth behind the Rust keyboard hook and WebView2 settings.

test("the context menu and print are blocked and accelerator keys are swallowed", async ({
  page,
}) => {
  await page.goto("/");
  // The guards install on mount; wait until `window.print` is the neutralised one.
  await page.waitForFunction(() => window.print.toString().replace(/\s/g, "") === "()=>{}");
  // The context menu is prevented.
  const menuDefaultPrevented = await page.evaluate(() => {
    const e = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    document.body.dispatchEvent(e);
    return e.defaultPrevented;
  });
  expect(menuDefaultPrevented).toBe(true);

  // Ctrl+P and F5 are cancelled before the browser acts on them.
  for (const combo of [
    { key: "p", ctrlKey: true },
    { key: "F5" },
    { key: "i", ctrlKey: true, shiftKey: true },
  ]) {
    const prevented = await page.evaluate((c) => {
      const e = new KeyboardEvent("keydown", { ...c, bubbles: true, cancelable: true });
      window.dispatchEvent(e);
      return e.defaultPrevented;
    }, combo);
    expect(prevented, `${JSON.stringify(combo)} is blocked`).toBe(true);
  }

  // A plain letter still types.
  const plainPrevented = await page.evaluate(() => {
    const e = new KeyboardEvent("keydown", { key: "a", bubbles: true, cancelable: true });
    window.dispatchEvent(e);
    return e.defaultPrevented;
  });
  expect(plainPrevented).toBe(false);

  // window.print does nothing (and does not throw).
  const printed = await page.evaluate(() => {
    let opened = false;
    window.addEventListener("beforeprint", () => (opened = true));
    window.print();
    return opened;
  });
  expect(printed).toBe(false);
});
