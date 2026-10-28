import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type { HubSnapshot, SessionStaff } from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// Settings → Branch → Logo (Story 6.7): a Manager picks a PNG or SVG, it is resized or refused
// here, and `settings.set` stores it under `kiosk.logo`.

const HUB = "http://127.0.0.1:7409";
const CORS = {
  "access-control-allow-origin": ORIGIN,
  "access-control-allow-methods": "GET, POST",
  "access-control-allow-headers": "authorization, content-type",
};
const STAFF: Record<string, SessionStaff> = {
  "manager-token": {
    staff_id: "01JMANAGER0000000000000000",
    display_name: "Manager",
    role_name: "Manager",
    permissions: ["settings.edit"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start", "pos.sell"],
  },
};
const KEY = "kiosk.logo";
const Buf = (globalThis as unknown as { Buffer: { from: (v: string | number[]) => never } }).Buffer;

const SVG_OK =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>';
const svgUri = (text: string) => "data:image/svg+xml;base64," + btoa(text);

type Sent = { key?: string; value?: string; base_rev?: number };
type Hub = {
  value: string;
  rev: number;
  intents: Sent[];
  refuse: { status: number; body: unknown } | null;
};

const newHub = (value = ""): Hub => ({ value, rev: 1, intents: [], refuse: null });

function snapshot(): HubSnapshot {
  return {
    sessions: [],
    branch: { name: "Dev Branch", branch_code: "DEV01" },
    groups: [],
    zones: [],
    devices: [],
    locale: null,
    ui: { idle_lock_minutes: 5, hc: false },
    connectivity: { state: "online", checked_at: null },
    setup: null,
    lan: null,
    paired: true,
  };
}

function json(route: Route, status: number, body: unknown) {
  return route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });
}

async function open(page: Page, hub: Hub, token = "manager-token") {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const url = new URL(req.url());
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (url.pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot());
      case "/ui/me":
        return STAFF[auth]
          ? json(route, 200, STAFF[auth])
          : json(route, 401, { code: "AUTH_FAILED", message: "no" });
      case "/ui/settings":
        return json(route, 200, [{ key: KEY, value: hub.value, rev: hub.rev }]);
      case "/ui/events":
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: "retry: 60000\n\n",
        });
      case "/ui/intent": {
        const { intent } = req.postDataJSON() as {
          intent: { base_rev?: number; payload: { key: string; value: string } };
        };
        hub.intents.push({ ...intent.payload, base_rev: intent.base_rev });
        if (hub.refuse) {
          const answer = hub.refuse;
          hub.refuse = null;
          return json(route, answer.status, answer.body);
        }
        if (!STAFF[auth].permissions.includes("settings.edit")) {
          return json(route, 403, { code: "PERMISSION_DENIED", message: "denied" });
        }
        if (intent.base_rev !== hub.rev) {
          return json(route, 409, { code: "STALE", message: "stale", field: "base_rev" });
        }
        hub.value = intent.payload.value;
        hub.rev += 1;
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default:
        return route.fulfill({ status: 404, headers: CORS });
    }
  });
  await page.goto("/settings/branch/logo");
  await expect(page.getByTestId("settings-logo")).toBeVisible();
}

const save = (page: Page) => page.getByRole("button", { name: en["hub.settings.save"] });
const remove = (page: Page) => page.getByRole("button", { name: en["hub.settings.logo.remove"] });
const file = (page: Page) => page.getByTestId("logo-file");
const error = (page: Page) => page.getByTestId("logo-error");

const pick = (page: Page, name: string, mimeType: string, content: string | number[]) =>
  file(page).setInputFiles({ name, mimeType, buffer: Buf.from(content) });

/** The bytes of a PNG drawn in the page. `noisy` fills it with incompressible pixels. */
async function pngBytes(page: Page, w: number, h: number, noisy = false): Promise<number[]> {
  return page.evaluate(
    async ([width, height, noise]) => {
      const c = document.createElement("canvas");
      c.width = width;
      c.height = height;
      const ctx = c.getContext("2d")!;
      const image = ctx.createImageData(width, height);
      for (let i = 0; i < image.data.length; i += 4) {
        image.data[i] = noise ? Math.random() * 256 : 30;
        image.data[i + 1] = noise ? Math.random() * 256 : 120;
        image.data[i + 2] = noise ? Math.random() * 256 : 200;
        image.data[i + 3] = 255;
      }
      ctx.putImageData(image, 0, 0);
      const blob = await new Promise<Blob>((r) => c.toBlob((b) => r(b!), "image/png"));
      return Array.from(new Uint8Array(await blob.arrayBuffer()));
    },
    [w, h, noisy] as const,
  );
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

test("the Logo section is in Branch settings and, with none set, shows the Arena 360 mark", async ({
  page,
}) => {
  await open(page, newHub());
  await expect(
    page.getByRole("link", { name: en["hub.settings.branch.nav.logo"], exact: true }),
  ).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("heading", { name: en["hub.settings.logo.title"] })).toBeVisible();
  await expect(page.getByTestId("logo-none")).toHaveText(en["hub.settings.logo.none"]);
  await expect(page.getByRole("img", { name: en["app.name"] })).toBeVisible();
  await expect(page.getByTestId("logo-preview")).toHaveCount(0);
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  await expect(remove(page)).toHaveAttribute("aria-disabled", "true");
  await expectNoSeriousAxe(page);
});

test("a chosen SVG is previewed and stored by settings.set under kiosk.logo with base_rev", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await pick(page, "logo.svg", "image/svg+xml", SVG_OK);
  const preview = page.getByTestId("logo-preview");
  await expect(preview).toHaveAttribute("src", svgUri(SVG_OK));
  await expect(page.getByText(en["hub.settings.logo.preview"])).toBeVisible();
  expect(hub.intents).toEqual([]);
  await save(page).click();
  await expect(page.getByTestId("logo-status")).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents).toEqual([
    { kind: "settings.set", key: KEY, value: svgUri(SVG_OK), base_rev: 1 },
  ]);
  expect(hub.value).toBe(svgUri(SVG_OK));
  await expect(page.getByText(en["hub.settings.logo.current"])).toBeVisible();
  await expect(page.getByTestId("logo-preview")).toHaveAttribute("src", svgUri(SVG_OK));
  await expect(remove(page)).not.toHaveAttribute("aria-disabled", "true");
  await expectNoSeriousAxe(page);
});

test("a PNG over 512 px is resized to at most 512 px and sent as a small PNG data URI", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await pick(page, "big.png", "image/png", await pngBytes(page, 1600, 900));
  const preview = page.getByTestId("logo-preview");
  await expect(preview).toBeVisible();
  const size = await preview.evaluate(async (img: HTMLImageElement) => {
    await img.decode();
    return { w: img.naturalWidth, h: img.naturalHeight, bytes: ((img.src.length - 22) * 3) / 4 };
  });
  expect(size.w).toBe(512);
  expect(size.h).toBe(288);
  expect(size.bytes).toBeLessThanOrEqual(200 * 1024);
  await save(page).click();
  await expect(page.getByTestId("logo-status")).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents).toHaveLength(1);
  expect(hub.intents[0].value!.startsWith("data:image/png;base64,")).toBe(true);
});

test("a PNG already within 512 px and 200 KB is sent unchanged", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  const bytes = await pngBytes(page, 200, 80);
  await pick(page, "small.png", "image/png", bytes);
  await expect(page.getByTestId("logo-preview")).toBeVisible();
  await save(page).click();
  await expect(page.getByTestId("logo-status")).toHaveText(en["hub.settings.saved"]);
  const sent = hub.intents[0].value!;
  const raw = atob(sent.slice("data:image/png;base64,".length));
  expect(Array.from(raw, (c) => c.charCodeAt(0))).toEqual(bytes);
});

test("a PNG that is still over 200 KB after resizing is refused before anything is sent", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await pick(page, "noise.png", "image/png", await pngBytes(page, 512, 512, true));
  await expect(error(page)).toContainText(en["hub.settings.logo.invalid.size"]);
  await expect(page.getByTestId("logo-preview")).toHaveCount(0);
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  expect(hub.intents).toEqual([]);
  await expectNoSeriousAxe(page);
});

test("an SVG with a script, a handler or a link to another file is refused", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  for (const bad of [
    "<svg><script>alert(1)</script></svg>",
    '<svg onload="x()"><rect/></svg>',
    "<svg><foreignObject><div/></foreignObject></svg>",
    '<svg><image href="https://example.com/a.png"/></svg>',
    '<svg><a href="javascript:alert(1)"/></svg>',
    '<svg><rect fill="url(https://example.com/g)"/></svg>',
    "<svg><!ENTITY x 'y'></svg>",
    "<html></html>",
  ]) {
    await pick(page, "logo.svg", "image/svg+xml", bad);
    await expect(error(page)).toContainText(en["hub.settings.logo.invalid.svg"]);
    await expect(page.getByTestId("logo-preview")).toHaveCount(0);
  }
  expect(hub.intents).toEqual([]);
  // An internal reference is fine.
  await pick(
    page,
    "logo.svg",
    "image/svg+xml",
    '<svg><defs><linearGradient id="g"/></defs><rect fill="url(#g)"/><use href="#g"/></svg>',
  );
  await expect(error(page)).toHaveText("");
  await expect(page.getByTestId("logo-preview")).toBeVisible();
});

test("a file that is not a PNG or SVG, or is a broken PNG, is refused with a reason", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await pick(page, "logo.jpg", "image/jpeg", "jpeg");
  await expect(error(page)).toContainText(en["hub.settings.logo.invalid.type"]);
  await pick(page, "broken.png", "image/png", "not a png");
  await expect(error(page)).toContainText(en["hub.settings.logo.invalid.read"]);
  expect(hub.intents).toEqual([]);
});

test("a Hub refusal shows the logo field error and leaves the logo as it was", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await pick(page, "logo.svg", "image/svg+xml", SVG_OK);
  hub.refuse = {
    status: 422,
    body: { code: "VALIDATION_FAILED", message: "the logo is over 200 KB", field: KEY },
  };
  await save(page).click();
  await expect(error(page)).toContainText(en["hub.settings.logo.invalid.server"]);
  expect(hub.value).toBe("");
  // The chosen file is still there to try again.
  await expect(page.getByTestId("logo-preview")).toBeVisible();
  await save(page).click();
  await expect(page.getByTestId("logo-status")).toHaveText(en["hub.settings.saved"]);
});

test("removing the logo stores an empty value and the mark shows again", async ({ page }) => {
  const hub = newHub(svgUri(SVG_OK));
  await open(page, hub);
  await expect(page.getByTestId("logo-preview")).toHaveAttribute("src", svgUri(SVG_OK));
  await remove(page).click();
  await expect(page.getByTestId("logo-status")).toHaveText(en["hub.settings.logo.removed"]);
  expect(hub.intents).toEqual([{ kind: "settings.set", key: KEY, value: "", base_rev: 1 }]);
  await expect(page.getByTestId("logo-none")).toBeVisible();
  await expect(page.getByTestId("logo-preview")).toHaveCount(0);
});

test("a change made elsewhere first is reported as stale", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await pick(page, "logo.svg", "image/svg+xml", SVG_OK);
  hub.rev = 2;
  await save(page).click();
  await expect(page.getByTestId("logo-status")).toContainText(en["hub.settings.branch.stale"]);
  expect(hub.value).toBe("");
});

test("without settings.edit the picker and buttons are disabled with the reason", async ({
  page,
}) => {
  await open(page, newHub(svgUri(SVG_OK)), "counter-token");
  await expect(file(page)).toBeDisabled();
  await expect(save(page)).toHaveAttribute("aria-disabled", "true");
  await expect(remove(page)).toHaveAttribute("aria-disabled", "true");
  await expect(page.getByText(en["hub.settings.branch.manager_only"]).first()).toBeVisible();
});
