import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";

// Accessibility floor for the shared @arena/ui components, checked on the gallery in every
// theme section. Each test names the offending element so a failure says what to fix.

const THEMES = ["dark", "hc", "light"] as const;
const INTERACTIVE = 'button, a, input, [role="tab"], [tabindex]';

/** A short, stable description of an element for failure messages. */
async function describe(locator: Locator): Promise<string> {
  return locator.evaluate((el) => {
    const theme = el.closest("[data-theme]")?.getAttribute("data-theme") ?? "root";
    const name =
      el.getAttribute("aria-label") ??
      (el.id ? document.querySelector(`label[for="${el.id}"]`)?.textContent : null) ??
      el.textContent ??
      "";
    const forced = el.getAttribute("data-force-state");
    return `[${theme}] <${el.tagName.toLowerCase()}${forced ? ` data-force-state=${forced}` : ""}> "${name.trim().slice(0, 40)}"`;
  });
}

async function openGallery(page: Page, query = "") {
  await page.goto(`/${query}`);
  await expect(page.locator("h1")).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
}

test.beforeEach(async ({ page }) => {
  await openGallery(page);
});

test("page has landmarks, one h1 and three theme sections", async ({ page }) => {
  await expect(page.locator("header")).toHaveCount(1);
  await expect(page.locator("main")).toHaveCount(1);
  await expect(page.locator("h1")).toHaveCount(1);
  for (const theme of THEMES) {
    await expect(page.locator(`main [data-theme="${theme}"]`)).toHaveCount(1);
  }
  // StatusBadge is Hub-only: dark and HC, never light.
  await expect(page.locator('[data-theme="dark"] [data-component="status-badge"]')).not.toHaveCount(
    0,
  );
  await expect(page.locator('[data-theme="hc"] [data-component="status-badge"]')).not.toHaveCount(
    0,
  );
  await expect(page.locator('[data-theme="light"] [data-component="status-badge"]')).toHaveCount(0);
});

test("tier-lock reads in text in every theme and its Upgrade link is a 44 px target", async ({
  page,
}) => {
  for (const theme of THEMES) {
    const locks = page.locator(`[data-theme="${theme}"] [data-component="tier-lock"]`);
    await expect(locks).toHaveCount(2);
    await expect(locks.first()).toContainText("Available on Basic");
    await expect(locks.first().getByRole("link", { name: "Upgrade" })).toHaveCount(1);
    const upgrade = locks.last().getByRole("link", { name: "Upgrade" });
    const box = await upgrade.boundingBox();
    expect(box?.height ?? 0).toBeGreaterThanOrEqual(44);
    await upgrade.focus();
    await expect(upgrade).toBeFocused();
  }
});

test("floor-tile: eight states in dark and HC, named by device, state and time", async ({
  page,
}) => {
  for (const theme of ["dark", "hc"] as const) {
    const section = page.locator(`[data-theme="${theme}"]`);
    const tiles = section.locator('[data-component="floor-tile"]');
    await expect(tiles).toHaveCount(8);
    const faces = tiles.getByRole("group");
    await expect(faces.nth(1)).toHaveAttribute("aria-label", "PC 2, In session, 01:23:45");
    await expect(faces.nth(0)).toHaveAttribute("aria-label", "PC 1, Free");
    for (let i = 0; i < 8; i++) {
      const box = await tiles.nth(i).boundingBox();
      expect(box?.height ?? 0).toBeGreaterThanOrEqual(128);
      await expect(tiles.nth(i).locator('[data-component="status-badge"]')).toBeVisible();
    }
    const last = tiles.nth(7);
    await expect(last.locator('[data-component="group-chip"]')).toHaveCount(0);
    await expect(last.locator(".bar")).toHaveCount(0);
    expect(await last.evaluate((el) => getComputedStyle(el).borderTopStyle)).toBe("dashed");
    await faces.nth(0).focus();
    await page.keyboard.press("Enter");
    await expect(section.getByTestId("tile-activated")).toHaveText("1");
  }
});

test("keyboard focus: every tab stop shows a visible indicator", async ({ page }) => {
  const indicator = (el: Element) => {
    const s = getComputedStyle(el);
    return {
      outline:
        s.outlineStyle === "none"
          ? "none"
          : `${s.outlineWidth} ${s.outlineStyle} ${s.outlineColor}`,
      shadow: s.boxShadow,
    };
  };
  const expected = await page.evaluate((selector) => {
    const seenGroups = new Set<string>();
    return [...document.querySelectorAll<HTMLElement>(selector)].filter((el) => {
      if (el.tabIndex < 0 || el.offsetParent === null) return false;
      // A radio group is one tab stop.
      if (el instanceof HTMLInputElement && el.type === "radio" && el.name) {
        if (seenGroups.has(el.name)) return false;
        seenGroups.add(el.name);
      }
      return true;
    }).length;
  }, INTERACTIVE);
  expect(expected).toBeGreaterThan(30);

  const seen: { label: string; focused: { outline: string; shadow: string }; forced: boolean }[] =
    [];
  await page.locator("body").focus();
  for (let i = 0; i < expected; i++) {
    await page.keyboard.press("Tab");
    const active = page.locator(":focus");
    await expect(active, `tab stop ${i + 1} has focus`).toHaveCount(1);
    const label = await describe(active);
    await active.evaluate((el, n) => el.setAttribute("data-tab-stop", String(n)), i);
    const focused = await active.evaluate(indicator);
    const forced = (await active.getAttribute("data-force-state")) === "focus";
    seen.push({ label, focused, forced });
    expect(
      focused.outline !== "none" || focused.shadow !== "none",
      `${label} has no focus indicator`,
    ).toBe(true);
  }

  // Compare against the same elements unfocused.
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  for (const [i, stop] of seen.entries()) {
    if (stop.forced) continue; // forced-focus samples draw the ring at rest by design
    const rest = await page.locator(`[data-tab-stop="${i}"]`).evaluate(indicator);
    expect(
      rest.outline !== stop.focused.outline || rest.shadow !== stop.focused.shadow,
      `${stop.label} looks the same focused and unfocused`,
    ).toBe(true);
  }
});

test("theme focus styles: dark double ring, HC 4px ring, light 2px ring", async ({ page }) => {
  const ring = async (theme: string) => {
    const field = page.locator(`[data-theme="${theme}"] input`).first();
    await field.focus();
    return field.evaluate((el) => {
      const s = getComputedStyle(el);
      return { width: s.outlineWidth, offset: s.outlineOffset, shadow: s.boxShadow };
    });
  };
  const dark = await ring("dark");
  expect(dark.width).toBe("2px");
  expect(dark.offset).toBe("2px");
  expect(dark.shadow).toContain("6px");
  const hc = await ring("hc");
  expect(hc.width).toBe("4px");
  expect(hc.shadow).toBe("none");
  const light = await ring("light");
  expect(light.width).toBe("2px");
  expect(light.shadow).toBe("none");
});

test("targets: every interactive element is at least 44 × 44 px in each theme", async ({
  page,
}) => {
  const controls = page.locator(INTERACTIVE);
  const count = await controls.count();
  expect(count).toBeGreaterThan(30);
  const small: string[] = [];
  for (let i = 0; i < count; i++) {
    const control = controls.nth(i);
    const box = await control.boundingBox();
    if (!box) continue; // not rendered
    if (box.width < 44 || box.height < 44) {
      small.push(`${await describe(control)} is ${box.width.toFixed(1)}×${box.height.toFixed(1)}`);
    }
  }
  expect(small, small.join("\n")).toEqual([]);
});

for (const theme of THEMES) {
  test(`axe: no serious or critical violations in the ${theme} section`, async ({ page }) => {
    const results = await new AxeBuilder({ page }).include(`[data-theme="${theme}"]`).analyze();
    const blocking = results.violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    );
    const report = blocking.map(
      (v) => `${v.id} (${v.impact}): ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
    );
    expect(report, report.join("\n")).toEqual([]);
  });
}

test("axe: no serious or critical violations on the whole page", async ({ page }) => {
  const results = await new AxeBuilder({ page }).analyze();
  const blocking = results.violations.filter(
    (v) => v.impact === "serious" || v.impact === "critical",
  );
  expect(blocking.map((v) => v.id)).toEqual([]);
});

test("HC: instant transitions, no shadows, 2px outlines that thicken to 3px on hover", async ({
  page,
}) => {
  await page.mouse.move(0, 0);
  const hc = page.locator('[data-theme="hc"]');
  const surfaces = hc.locator('button, input, [role="tab"]');
  const count = await surfaces.count();
  expect(count).toBeGreaterThan(10);
  const problems: string[] = [];
  for (let i = 0; i < count; i++) {
    const el = surfaces.nth(i);
    const s = await el.evaluate((node) => {
      const c = getComputedStyle(node);
      return {
        transition: c.transitionDuration,
        shadow: c.boxShadow,
        outline: `${c.outlineWidth} ${c.outlineStyle}`,
        forced: node.getAttribute("data-force-state"),
      };
    });
    const label = await describe(el);
    if (s.transition.split(",").some((d) => d.trim() !== "0s")) {
      problems.push(`${label} transition ${s.transition}`);
    }
    if (s.shadow !== "none") problems.push(`${label} box-shadow ${s.shadow}`);
    const want =
      s.forced === "focus" ? "4px solid" : s.forced === "hover" ? "3px solid" : "2px solid";
    if (s.outline !== want) problems.push(`${label} outline ${s.outline}, want ${want}`);
  }
  expect(problems, problems.join("\n")).toEqual([]);

  // A real pointer hover thickens the outline to 3px.
  const button = hc.locator('button:not([data-force-state]):not([aria-disabled="true"])').first();
  await button.hover();
  await expect(button).toHaveCSS("outline-width", "3px");
  await page.mouse.move(0, 0);
  await expect(button).toHaveCSS("outline-width", "2px");
});

test("pseudo-locale: no clipped or overlapping text", async ({ page }) => {
  await openGallery(page, "?pseudo=1");
  await expect(page.locator("h1")).toContainText("[");

  const clipped = await page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>("body *")]
      .filter((el) => {
        const s = getComputedStyle(el);
        const clips = [s.overflowX, s.overflowY].some((o) => o === "hidden" || o === "clip");
        // Visually-hidden live regions are 1px boxes by design.
        if (!clips || el.clientWidth <= 1) return false;
        return el.scrollWidth > el.clientWidth || el.scrollHeight > el.clientHeight;
      })
      .map((el) => `${el.tagName.toLowerCase()}.${el.className} "${el.textContent?.slice(0, 40)}"`),
  );
  expect(clipped, clipped.join("\n")).toEqual([]);

  const overlaps = await page.evaluate(() => {
    const boxes = [...document.querySelectorAll<HTMLElement>("[data-component]")];
    const out: string[] = [];
    for (let i = 0; i < boxes.length; i++) {
      for (let j = i + 1; j < boxes.length; j++) {
        const a = boxes[i];
        const b = boxes[j];
        if (a.contains(b) || b.contains(a)) continue;
        const r1 = a.getBoundingClientRect();
        const r2 = b.getBoundingClientRect();
        const x = Math.min(r1.right, r2.right) - Math.max(r1.left, r2.left);
        const y = Math.min(r1.bottom, r2.bottom) - Math.max(r1.top, r2.top);
        if (x > 0.5 && y > 0.5) {
          out.push(
            `${a.dataset.component} "${a.textContent?.trim().slice(0, 30)}" overlaps ${b.dataset.component} "${b.textContent?.trim().slice(0, 30)}"`,
          );
        }
      }
    }
    return out;
  });
  expect(overlaps, overlaps.join("\n")).toEqual([]);

  // Text inside each component stays inside its box.
  const spills = await page.evaluate(() =>
    [...document.querySelectorAll<HTMLElement>("[data-component] button, [data-component] .pill")]
      .filter((el) => el.scrollWidth > el.clientWidth + 1)
      .map((el) => `${el.tagName.toLowerCase()} "${el.textContent?.trim().slice(0, 40)}"`),
  );
  expect(spills, spills.join("\n")).toEqual([]);
});

test("disabled buttons: 40% opacity, focusable, and a non-empty reason linked by aria-describedby", async ({
  page,
}) => {
  await expect(page.locator("button:disabled")).toHaveCount(0);
  const disabled = page.locator('button[aria-disabled="true"]');
  const count = await disabled.count();
  expect(count).toBeGreaterThanOrEqual(9);
  for (let i = 0; i < count; i++) {
    const button = disabled.nth(i);
    const label = await describe(button);
    await expect(button, label).toHaveCSS("opacity", "0.4");
    await expect(button, label).toHaveCSS("cursor", "not-allowed");
    const reasonId = await button.getAttribute("aria-describedby");
    expect(reasonId, `${label} has aria-describedby`).toBeTruthy();
    const reason = page.locator(`[id="${reasonId}"]`);
    await expect(reason, label).toBeVisible();
    expect((await reason.textContent())?.trim(), `${label} reason text`).toBeTruthy();
    await expect(reason, `${label} reason at full ink`).toHaveCSS("opacity", "1");
    // Stays a tab stop so the reason is reachable, and shows the focus ring.
    expect(await button.evaluate((el) => el.tabIndex), `${label} tabIndex`).toBe(0);
    await button.focus();
    await expect(button, `${label} focus ring`).not.toHaveCSS("outline-style", "none");
  }
});

test("disabled buttons cannot be activated by click or keyboard", async ({ page }) => {
  await page.evaluate(() => {
    (window as unknown as { activations: boolean[] }).activations = [];
    // Capture phase sees every click; read `defaultPrevented` once dispatch has finished.
    window.addEventListener(
      "click",
      (e) =>
        setTimeout(() =>
          (window as unknown as { activations: boolean[] }).activations.push(e.defaultPrevented),
        ),
      true,
    );
  });
  const button = page.locator('[data-theme="dark"] button[aria-disabled="true"]').first();
  await button.click({ force: true }); // Playwright treats aria-disabled as not actionable
  await button.focus();
  await page.keyboard.press("Enter");
  await page.keyboard.press("Space");
  await page.waitForTimeout(50);
  const activations = await page.evaluate(
    () => (window as unknown as { activations: boolean[] }).activations,
  );
  expect(activations.length).toBeGreaterThan(0);
  expect(activations.every(Boolean), "every click on a disabled button is cancelled").toBe(true);
});

test("disabled button hover does not tint", async ({ page }) => {
  const button = page.locator('[data-theme="dark"] button[aria-disabled="true"]').first();
  const before = await button.evaluate((el) => getComputedStyle(el).backgroundColor);
  await button.hover();
  await page.waitForTimeout(200);
  await expect(button).toHaveCSS("background-color", before);
});

for (const theme of THEMES) {
  test(`hover and pressed states in the ${theme} section`, async ({ page }) => {
    await page.mouse.move(0, 0);
    const buttons = page.locator(`[data-theme="${theme}"] button.button--secondary`);
    const rest = buttons.locator(":scope:not([data-force-state]):not([aria-disabled])").first();
    const hover = buttons.and(page.locator('[data-force-state="hover"]')).first();
    const pressed = buttons.and(page.locator('[data-force-state="pressed"]')).first();
    const css = (l: Locator) =>
      l.evaluate((el) => {
        const s = getComputedStyle(el);
        return {
          bg: s.backgroundColor,
          shadow: s.boxShadow,
          outlineWidth: s.outlineWidth,
          outlineOffset: s.outlineOffset,
        };
      });
    const r = await css(rest);
    const h = await css(hover);
    const p = await css(pressed);
    if (theme === "hc") {
      // HC: no tints; hover thickens the outline and pressed insets it.
      expect(h.bg).toBe(r.bg);
      expect(p.bg).toBe(r.bg);
      expect(h.outlineWidth).toBe("3px");
      expect(p.outlineOffset).toBe("-2px");
      expect(p.shadow).toBe("none");
    } else {
      expect(h.bg, "hover tints").not.toBe(r.bg);
      expect(p.bg, "pressed tints").not.toBe(r.bg);
      // Light hover is 6% darker, the same as pressed, so only dark separates them by fill.
      if (theme === "dark") expect(p.bg, "pressed differs from hover").not.toBe(h.bg);
      expect(p.shadow).toContain("inset");
    }
    // A real pointer hover matches the forced hover sample.
    await rest.hover();
    await expect(rest).toHaveCSS("background-color", h.bg);
    await expect(rest).toHaveCSS("outline-width", theme === "hc" ? "3px" : h.outlineWidth);
  });
}

test("field errors: ⚠, message and aria-invalid; submit focuses the first invalid field", async ({
  page,
}) => {
  const dark = page.locator('[data-theme="dark"]');
  const errored = dark.locator('input[aria-invalid="true"]').first();
  const errorId = await errored.getAttribute("aria-describedby");
  const message = page.locator(`[id="${errorId}"]`);
  await expect(message).toContainText("⚠");
  await expect(message).toContainText("10-digit");
  await expect(errored).toHaveCSS("border-top-width", "2px");

  const form = dark.locator("form");
  await form.getByRole("button", { name: "Sign in" }).click();
  const username = form.getByLabel("Username");
  await expect(username).toBeFocused();
  await expect(username).toHaveAttribute("aria-invalid", "true");
  await expect(form.locator('input[aria-invalid="true"]')).toHaveCount(2);
  await expect(form.locator('[aria-live="polite"]').first()).toContainText("required");

  // Fixing the first field moves focus to the next invalid one on submit.
  await username.fill("neha");
  await form.getByRole("button", { name: "Sign in" }).click();
  await expect(form.getByLabel("Password", { exact: true })).toBeFocused();
  await expect(username).not.toHaveAttribute("aria-invalid", "true");
});

test("password: show/hide toggles the input type and the label; paste works", async ({ page }) => {
  const form = page.locator('[data-theme="dark"] form');
  const input = form.getByLabel("Password", { exact: true });
  const toggle = form.getByRole("button", { name: "Show password" });
  await expect(input).toHaveAttribute("type", "password");
  await toggle.click();
  await expect(input).toHaveAttribute("type", "text");
  await expect(form.getByRole("button", { name: "Hide password" })).toBeVisible();
  await form.getByRole("button", { name: "Hide password" }).click();
  await expect(input).toHaveAttribute("type", "password");

  const pasted = await input.evaluate((el: HTMLInputElement) => {
    const data = new DataTransfer();
    data.setData("text/plain", "hunter2");
    const event = new ClipboardEvent("paste", { clipboardData: data, cancelable: true });
    el.dispatchEvent(event);
    return !event.defaultPrevented;
  });
  expect(pasted, "paste is never blocked").toBe(true);
});

test("tabs: arrows wrap, Home/End jump, roving tabindex follows selection", async ({ page }) => {
  const tablist = page.locator('[data-theme="dark"] [role="tablist"]').first();
  const tabs = tablist.getByRole("tab");
  await expect(tabs).toHaveCount(3);
  const last = tabs.nth(2);
  await last.click();
  await expect(last).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowRight");
  await expect(tabs.nth(0)).toBeFocused();
  await expect(tabs.nth(0)).toHaveAttribute("aria-selected", "true");
  await expect(last).toHaveAttribute("aria-selected", "false");
  await page.keyboard.press("ArrowLeft");
  await expect(last).toBeFocused();
  await page.keyboard.press("Home");
  await expect(tabs.nth(0)).toBeFocused();
  await page.keyboard.press("End");
  await expect(last).toBeFocused();
  for (let i = 0; i < 3; i++) {
    await expect(tabs.nth(i)).toHaveAttribute("tabindex", i === 2 ? "0" : "-1");
  }
  const panelId = await last.getAttribute("aria-controls");
  await expect(page.locator(`[id="${panelId}"]`)).toContainText("yearly");
});

test("badges: live regions exist for the AlertBadge count and TextField errors", async ({
  page,
}) => {
  for (const theme of THEMES) {
    const section = page.locator(`[data-theme="${theme}"]`);
    const alertRegion = section.locator(
      '.row [data-component="alert-badge"] [aria-live="polite"][role="status"]',
    );
    await expect(alertRegion).toHaveCount(1);
    await expect(alertRegion).toHaveText("3 alerts");
    await section.getByRole("button", { name: "Add alert" }).click();
    await expect(alertRegion).toHaveText("4 alerts");
    const fields = section.locator('[data-component="text-field"]');
    const regions = section.locator('[data-component="text-field"] [aria-live="polite"]');
    await expect(regions).toHaveCount(await fields.count());
  }
});

test("links are underlined", async ({ page }) => {
  const links = page.locator("main a");
  expect(await links.count()).toBeGreaterThan(0);
  for (const link of await links.all()) {
    await expect(link).toHaveCSS("text-decoration-line", "underline");
  }
});
