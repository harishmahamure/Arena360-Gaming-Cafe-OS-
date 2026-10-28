import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";
import type {
  DeviceScanView,
  HubSnapshot,
  LibraryEntryView,
  LibraryInstall,
  ProposalView,
  SessionStaff,
} from "@arena/ui/generated";
import en from "../../../packages/ui/src/messages/en.json" with { type: "json" };
import { ORIGIN } from "./origin";

// Node's Buffer, without the node typings this project does not load.
const nodeBuffer = (globalThis as unknown as { Buffer: { from(s: string, e: string): Uint8Array } })
  .Buffer;
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
    permissions: ["games.manage"],
  },
  "counter-token": {
    staff_id: "01JCOUNTER0000000000000000",
    display_name: "Counter",
    role_name: "Counter",
    permissions: ["session.start"],
  },
};

type Payload = Record<string, unknown> & { kind: string };
type Hub = {
  entries: LibraryEntryView[];
  /** Every paired PC with its last scan (Story 6.4). */
  scans: DeviceScanView[];
  /** The scan list answers 500 while set. */
  scansDown?: boolean;
  /** What "Found on PCs" lists (Story 6.5), and how many are dismissed. */
  proposals: ProposalView[];
  dismissed: number;
  /** The proposals list answers 500 while set. */
  proposalsDown?: boolean;
  /** A `library.upsert` carrying this catalogue id is refused on its path. */
  refuseCatalog?: string;
  /** Stored covers by hash, served at `/ui/library/cover/<hash>`. */
  covers: Record<string, string>;
  intents: { payload: Payload; base_rev?: number; ticket?: string }[];
  elevations: { items: { intent_kind: string }[]; pin: string }[];
  /** The field a save is refused on (`VALIDATION_FAILED`), if any. */
  refuse?: string;
  stale: boolean;
  /** Pushed on the next event stream the page opens. */
  push: string[];
};

const newHub = (): Hub => ({
  entries: [],
  scans: [],
  proposals: [],
  dismissed: 0,
  covers: {},
  intents: [],
  elevations: [],
  stale: false,
  push: [],
});

const entry = (over: Partial<LibraryEntryView> = {}): LibraryEntryView => ({
  id: "L1",
  kind: "game",
  name: "Counter-Strike 2",
  launcher: "steam",
  executable_path: "C:\\Games\\CS2\\cs2.exe",
  launch_uri: "",
  launch_args: "",
  category: "Shooter",
  tags: ["fps", "esports"],
  age_rating: "18+",
  enabled: true,
  catalog_id: null,
  installs: [],
  rev: 3,
  ...over,
});

const snapshot = (): HubSnapshot => ({
  branch: { name: "Dev Branch", branch_code: "DEV01" },
  groups: [],
  zones: [],
  devices: [],
  locale: {
    currency: "INR",
    exponent: 2,
    symbol: "₹",
    symbol_position: "before",
    number_format: "indian",
    tz: "Asia/Kolkata",
    day_start_min: 360,
    fy_start_month: 4,
    pending: null,
  },
  ui: { idle_lock_minutes: 5, hc: false },
  connectivity: { state: "online", checked_at: null },
  lan: null,
  paired: true,
});

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({
    status,
    contentType: "application/json",
    headers: CORS,
    body: JSON.stringify(body),
  });

async function open(page: Page, hub: Hub, token = "manager-token") {
  await page.addInitScript((t) => sessionStorage.setItem("arena.hub.token", t), token);
  await page.route(`${HUB}/**`, async (route) => {
    const req = route.request();
    if (req.method() === "OPTIONS") return route.fulfill({ status: 204, headers: CORS });
    const auth = (req.headers()["authorization"] ?? "").replace(/^Bearer /, "");
    switch (new URL(req.url()).pathname) {
      case "/ui/snapshot":
        return json(route, 200, snapshot());
      case "/ui/me":
        return json(route, 200, STAFF[auth]);
      case "/ui/library":
        return json(route, 200, { entries: hub.entries });
      case "/ui/library/scans":
        return hub.scansDown
          ? json(route, 500, { code: "INTERNAL", message: "no" })
          : json(route, 200, { devices: hub.scans });
      case "/ui/library/proposals":
        return hub.proposalsDown
          ? json(route, 500, { code: "INTERNAL", message: "no" })
          : json(route, 200, { proposals: hub.proposals, dismissed: hub.dismissed });
      case "/ui/events": {
        const pushed = hub.push.splice(0).map((e) => `event: ${e}\ndata: {}\n\n`);
        return route.fulfill({
          status: 200,
          contentType: "text/event-stream",
          headers: CORS,
          body: `retry: 300\n\n${pushed.join("")}`,
        });
      }
      case "/ui/elevate": {
        const body = req.postDataJSON() as Hub["elevations"][number];
        hub.elevations.push(body);
        if (body.pin !== "2222") return json(route, 403, { code: "PIN_REJECTED", message: "no" });
        return json(route, 200, { ticket: "T1", items: body.items, expires_in_s: 60 });
      }
      case "/ui/intent": {
        const body = req.postDataJSON() as {
          intent: { payload: Payload; base_rev?: number };
          elevation_ticket?: string;
        };
        const { payload } = body.intent;
        hub.intents.push({
          payload,
          base_rev: body.intent.base_rev,
          ticket: body.elevation_ticket,
        });
        if (hub.refuse) {
          return json(route, 422, {
            code: "VALIDATION_FAILED",
            message: "no",
            field: hub.refuse,
          });
        }
        if (hub.stale) {
          return json(route, 409, { code: "STALE", message: "changed", field: "base_rev" });
        }
        if (payload.kind === "library.dismiss") {
          const keys = payload.keys as string[];
          hub.proposals = hub.proposals.filter((p) => !keys.includes(p.key));
          hub.dismissed += keys.length;
          return json(route, 200, { applied: { intent_id: "I", changes: [] } });
        }
        if (payload.kind === "library.dismiss_clear") {
          hub.dismissed = 0;
          return json(route, 200, { applied: { intent_id: "I", changes: [] } });
        }
        if (
          payload.kind === "library.upsert" &&
          hub.refuseCatalog !== undefined &&
          payload.catalog_id === hub.refuseCatalog
        ) {
          return json(route, 422, {
            code: "VALIDATION_FAILED",
            message: "no",
            field: "executable_path",
          });
        }
        if (payload.kind === "library.upsert" && Array.isArray(payload.installs)) {
          const installs = payload.installs as { match_key: string }[];
          hub.proposals = hub.proposals.filter((p) => !installs.some((i) => i.match_key === p.key));
        }
        if (payload.kind === "library.delete") {
          hub.entries = hub.entries.filter((e) => e.id !== payload.id);
        } else {
          const existing = hub.entries.find((e) => e.id === payload.id);
          const next: LibraryEntryView = {
            id: (payload.id as string | undefined) ?? `L${hub.entries.length + 1}`,
            kind: payload.entry_kind as string,
            name: payload.name as string,
            launcher: payload.launcher as string,
            executable_path: payload.executable_path as string,
            launch_uri: payload.launch_uri as string,
            launch_args: payload.launch_args as string,
            category: payload.category as string,
            tags: payload.tags as string[],
            age_rating: payload.age_rating as string,
            enabled: payload.enabled as boolean,
            catalog_id: (payload.catalog_id as string | undefined) ?? null,
            installs:
              (payload.installs as LibraryInstall[] | undefined) ?? existing?.installs ?? [],
            rev: (existing?.rev ?? 0) + 1,
          };
          if (existing) Object.assign(existing, next);
          else hub.entries.push(next);
        }
        return json(route, 200, { applied: { intent_id: "I", changes: [] } });
      }
      default: {
        const cover = /^\/ui\/library\/cover\/([0-9a-f]{64})$/.exec(new URL(req.url()).pathname);
        if (cover && hub.covers[cover[1]]) {
          return route.fulfill({
            status: 200,
            contentType: "image/png",
            headers: CORS,
            body: nodeBuffer.from(hub.covers[cover[1]], "base64") as never,
          });
        }
        return route.fulfill({ status: 404, headers: CORS });
      }
    }
  });
  await page.goto("/settings/games");
  await expect(page.getByTestId("game-form")).toBeVisible();
}

async function expectNoSeriousAxe(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
  expect(bad.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual(
    [],
  );
}

const add = (page: Page) => page.getByRole("button", { name: en["hub.games.add"] });
const save = (page: Page) => page.getByRole("button", { name: en["hub.games.save"] });
const status = (page: Page) => page.getByTestId("game-status");

test("a manager adds a game with its launcher, path, category, tags and age rating", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub);
  await expectNoSeriousAxe(page);

  await page.getByLabel(en["hub.games.name"], { exact: true }).fill(" Valorant ");
  await page.getByLabel(en["hub.games.launcher"]).selectOption("riot");
  await page.getByLabel(en["hub.games.exe"]).fill("C:\\Riot Games\\VALORANT\\live\\val.exe");
  await page.getByLabel(en["hub.games.uri"]).fill("riot://launch");
  await page.getByLabel(en["hub.games.args"]).fill("--fullscreen");
  await page.getByLabel(en["hub.games.category"]).fill("Shooter");
  await page.getByLabel(en["hub.games.tags"]).fill("fps, team , FPS, tactical");
  await page.getByLabel(en["hub.games.age"], { exact: true }).fill("16+");
  await add(page).click();
  await expect(status(page)).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents[0].payload).toEqual({
    kind: "library.upsert",
    entry_kind: "game",
    name: "Valorant",
    launcher: "riot",
    executable_path: "C:\\Riot Games\\VALORANT\\live\\val.exe",
    launch_uri: "riot://launch",
    launch_args: "--fullscreen",
    category: "Shooter",
    tags: ["fps", "team", "tactical"],
    age_rating: "16+",
    enabled: true,
  });
  expect(hub.intents[0].base_rev).toBeUndefined();

  const row = page.getByTestId("game-row");
  await expect(row).toHaveCount(1);
  await expect(page.getByTestId("game-name")).toHaveText("Valorant");
  await expect(page.getByTestId("game-launcher")).toHaveText(en["hub.games.launcher.riot"]);
  await expect(page.getByTestId("game-path")).toHaveText("C:\\Riot Games\\VALORANT\\live\\val.exe");
  await expect(page.getByTestId("game-tags")).toHaveText("fps, team, tactical");
  await expectNoSeriousAxe(page);
});

test("an app with only a launch link needs no path and can be hidden", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await page.getByRole("radio", { name: en["hub.games.kind.app"] }).check();
  await page.getByLabel(en["hub.games.name"], { exact: true }).fill("Spotify");
  await page.getByLabel(en["hub.games.launcher"]).selectOption("none");
  await page.getByLabel(en["hub.games.uri"]).fill("spotify://");
  await page.getByLabel(en["hub.games.enabled"]).uncheck();
  await add(page).click();
  await expect(status(page)).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents[0].payload).toMatchObject({
    kind: "library.upsert",
    entry_kind: "app",
    executable_path: "",
    launch_uri: "spotify://",
    enabled: false,
  });
  await expect(page.getByTestId("game-row")).toContainText(en["hub.games.badge.disabled"]);
});

test("bad input shows its message in place and sends nothing", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  // A name of spaces passes the browser's required check but is empty once trimmed.
  await page.getByLabel(en["hub.games.name"], { exact: true }).fill("   ");
  await add(page).click();
  await expect(page.getByTestId("game-form")).toContainText(en["hub.games.invalid.name"]);
  await expect(page.getByTestId("game-form")).toContainText(
    en["hub.games.invalid.executable_path"],
  );

  await page.getByLabel(en["hub.games.name"], { exact: true }).fill("Game");
  await page.getByLabel(en["hub.games.exe"]).fill("games\\g.exe");
  await add(page).click();
  await expect(page.getByLabel(en["hub.games.exe"])).toHaveAttribute("aria-invalid", "true");

  await page.getByLabel(en["hub.games.exe"]).fill("C:\\Games\\*.exe");
  await add(page).click();
  await expect(page.getByLabel(en["hub.games.exe"])).toHaveAttribute("aria-invalid", "true");

  await page.getByLabel(en["hub.games.exe"]).fill("C:\\Games\\g.exe");
  await page.getByLabel(en["hub.games.uri"]).fill("nonsense");
  await add(page).click();
  await expect(page.getByTestId("game-form")).toContainText(en["hub.games.invalid.launch_uri"]);

  await page.getByLabel(en["hub.games.uri"]).fill("");
  await page.getByLabel(en["hub.games.tags"]).fill("1,2,3,4,5,6,7,8,9,10,11");
  await add(page).click();
  await expect(page.getByTestId("game-form")).toContainText(en["hub.games.invalid.tags"]);
  expect(hub.intents).toHaveLength(0);
});

test("the Hub's field refusal lands on that field", async ({ page }) => {
  const hub = newHub();
  hub.refuse = "executable_path";
  await open(page, hub);
  await page.getByLabel(en["hub.games.name"], { exact: true }).fill("Game");
  await page.getByLabel(en["hub.games.exe"]).fill("C:\\Games\\g.exe");
  await add(page).click();
  await expect(page.getByTestId("game-form")).toContainText(
    en["hub.games.invalid.executable_path"],
  );
  expect(hub.intents).toHaveLength(1);
});

test("editing sends the rev and keeps the fields it was not asked to change", async ({ page }) => {
  const hub = newHub();
  hub.entries = [entry()];
  await open(page, hub);
  await expect(page.getByTestId("game-name")).toHaveText("Counter-Strike 2");
  await page.getByRole("button", { name: "Edit Counter-Strike 2" }).click();
  await expect(page.getByLabel(en["hub.games.name"], { exact: true })).toHaveValue(
    "Counter-Strike 2",
  );
  await expect(page.getByLabel(en["hub.games.tags"])).toHaveValue("fps, esports");
  await page.getByLabel(en["hub.games.category"]).fill("Action");
  await page.getByLabel(en["hub.games.enabled"]).uncheck();
  await save(page).click();
  await expect(status(page)).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents[0].base_rev).toBe(3);
  expect(hub.intents[0].payload).toMatchObject({
    kind: "library.upsert",
    id: "L1",
    name: "Counter-Strike 2",
    category: "Action",
    tags: ["fps", "esports"],
    enabled: false,
  });
  await expect(page.getByTestId("game-row")).toContainText(en["hub.games.badge.disabled"]);
});

test("a catalogue-linked entry shows its name, launcher and age rating read-only", async ({
  page,
}) => {
  const hub = newHub();
  hub.entries = [entry({ catalog_id: "01JCATALOG" })];
  await open(page, hub);
  await expect(page.getByTestId("game-row")).toContainText(en["hub.games.badge.catalog"]);
  await page.getByRole("button", { name: "Edit Counter-Strike 2" }).click();
  await expect(page.getByTestId("game-linked")).toBeVisible();
  await expect(page.getByTestId("game-locked-name")).toHaveText("Counter-Strike 2");
  await expect(page.getByTestId("game-locked-launcher")).toHaveText(en["hub.games.launcher.steam"]);
  await expect(page.getByTestId("game-locked-age")).toHaveText("18+");
  await expect(page.getByLabel(en["hub.games.name"], { exact: true })).toHaveCount(0);
  await expect(page.getByLabel(en["hub.games.launcher"])).toHaveCount(0);
  await expect(page.getByLabel(en["hub.games.age"], { exact: true })).toHaveCount(0);
  // Category and tags stay editable.
  await page.getByLabel(en["hub.games.category"]).fill("Competitive");
  await page.getByLabel(en["hub.games.tags"]).fill("ranked");
  await save(page).click();
  await expect(status(page)).toHaveText(en["hub.settings.saved"]);
  expect(hub.intents[0].payload).toMatchObject({
    id: "L1",
    name: "Counter-Strike 2",
    launcher: "steam",
    age_rating: "18+",
    catalog_id: "01JCATALOG",
    category: "Competitive",
    tags: ["ranked"],
  });
});

test("a stale edit reloads the entry and asks to try again", async ({ page }) => {
  const hub = newHub();
  hub.entries = [entry()];
  hub.stale = true;
  await open(page, hub);
  await page.getByRole("button", { name: "Edit Counter-Strike 2" }).click();
  await save(page).click();
  await expect(status(page)).toContainText(en["hub.games.stale"]);
});

test("removing an entry sends library.delete with its rev", async ({ page }) => {
  const hub = newHub();
  hub.entries = [entry()];
  await open(page, hub);
  await page.getByRole("button", { name: "Remove Counter-Strike 2" }).click();
  await expect(status(page)).toHaveText(en["hub.games.removed"]);
  expect(hub.intents[0]).toMatchObject({
    payload: { kind: "library.delete", id: "L1" },
    base_rev: 3,
  });
  await expect(page.getByTestId("game-row")).toHaveCount(0);
  await expect(page.getByText(en["hub.games.empty"])).toBeVisible();
});

test("the list reloads when the Hub pushes library.changed", async ({ page }) => {
  const hub = newHub();
  hub.entries = [entry()];
  await open(page, hub);
  await expect(page.getByTestId("game-row")).toHaveCount(1);
  hub.entries = [
    entry(),
    entry({ id: "L2", name: "Apex", executable_path: "C:\\Games\\apex.exe" }),
  ];
  hub.push.push("library.changed");
  await expect(page.getByTestId("game-row")).toHaveCount(2, { timeout: 10_000 });
});

test("a Counter's save asks a manager's PIN in place and is applied with the ticket", async ({
  page,
}) => {
  const hub = newHub();
  await open(page, hub, "counter-token");
  await page.getByLabel(en["hub.games.name"], { exact: true }).fill("Dota 2");
  await page.getByLabel(en["hub.games.exe"]).fill("C:\\Games\\dota.exe");
  await add(page).click();
  await expect(page.getByTestId("game-approval")).toBeVisible();
  expect(hub.intents).toHaveLength(0);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(status(page)).toHaveText(en["hub.settings.saved"]);
  expect(hub.elevations.at(-1)).toEqual({
    items: [{ intent_kind: "library.upsert" }],
    pin: "2222",
  });
  expect(hub.intents[0].ticket).toBe("T1");
});

// ---- Story 6.4: each PC's last scan ----

const scanned = (over: Partial<DeviceScanView> = {}): DeviceScanView => ({
  device_id: "D1",
  name: "PC-01",
  scanned_at: Date.UTC(2026, 9, 29, 23, 35),
  entry_count: 42,
  skipped: 0,
  launchers: [
    { launcher: "steam", path: "C:\\Program Files (x86)\\Steam" },
    { launcher: "riot", path: "C:\\Riot Games\\Riot Client" },
  ],
  ...over,
});

/** The branch-time text the page shows for `at` (the page formats in the branch zone). */
const branchTime = (at: number) =>
  new Intl.DateTimeFormat("en-US", {
    dateStyle: "medium",
    timeStyle: "short",
    timeZone: "Asia/Kolkata",
  }).format(at);

test("Settings > Games shows each PC's last scan time, what it found and never-scanned PCs", async ({
  page,
}) => {
  const hub = newHub();
  hub.scans = [
    scanned(),
    scanned({
      device_id: "D2",
      name: "PC-02",
      scanned_at: null,
      entry_count: null,
      skipped: null,
      launchers: null,
    }),
    scanned({
      device_id: "D3",
      name: "PC-03",
      entry_count: 0,
      skipped: 4,
      launchers: [],
    }),
  ];
  await open(page, hub);
  await expect(page.getByTestId("scan-row")).toHaveCount(3);
  const first = page.getByTestId("scan-row").nth(0);
  await expect(first.getByTestId("scan-name")).toHaveText("PC-01");
  await expect(first.getByTestId("scan-last")).toHaveText(
    en["hub.games.scans.last"].replace("{date}", branchTime(Date.UTC(2026, 9, 29, 23, 35))),
  );
  await expect(first.getByTestId("scan-count")).toHaveText(
    en["hub.games.scans.found"].replace("{count}", "42"),
  );
  await expect(first.getByTestId("scan-launchers")).toHaveText(
    en["hub.games.scans.launchers"].replace(
      "{names}",
      `${en["hub.games.launcher.steam"]}, ${en["hub.games.launcher.riot"]}`,
    ),
  );
  await expect(first.getByTestId("scan-skipped")).toHaveCount(0);
  const never = page.getByTestId("scan-row").nth(1);
  await expect(never.getByTestId("scan-last")).toHaveText(en["hub.games.scans.never"]);
  await expect(never.getByTestId("scan-count")).toHaveCount(0);
  const last = page.getByTestId("scan-row").nth(2);
  await expect(last.getByTestId("scan-launchers")).toHaveText(en["hub.games.scans.no_launchers"]);
  await expect(last.getByTestId("scan-skipped")).toHaveText(
    en["hub.games.scans.skipped"].replace("{count}", "4"),
  );
  await expectNoSeriousAxe(page);
});

test("the scan list reloads when the Hub pushes library.changed after a PC reports", async ({
  page,
}) => {
  const hub = newHub();
  hub.scans = [scanned({ scanned_at: null, entry_count: null, skipped: null, launchers: null })];
  await open(page, hub);
  await expect(page.getByTestId("scan-last")).toHaveText(en["hub.games.scans.never"]);
  hub.scans = [scanned()];
  hub.push.push("library.changed");
  await expect(page.getByTestId("scan-count")).toHaveText(
    en["hub.games.scans.found"].replace("{count}", "42"),
    { timeout: 10_000 },
  );
});

test("no paired PCs, and a scan list that cannot be loaded, say so", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await expect(page.getByTestId("scan-empty")).toHaveText(en["hub.games.scans.empty"]);
  const down = newHub();
  down.scansDown = true;
  await page.unrouteAll();
  await open(page, down);
  await expect(page.getByText(en["hub.games.scans.load_failed"])).toBeVisible();
});

// ---- Story 6.5: Found on PCs ----

const SHA = "ab".repeat(32);
/** A 1×1 PNG. */
const PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

const proposal = (over: Partial<ProposalView> = {}): ProposalView => ({
  key: "game:G1",
  name: "Great Game",
  file_name: "g.exe",
  product_name: "Great",
  publisher: "Maker",
  catalog_id: "G1",
  cover_sha256: null,
  launcher: "steam",
  category: "Shooter",
  age_rating: "16+",
  pc_count: 2,
  devices: [
    { device_id: "D1", label: "PC-1", path: "C:\\Games\\g.exe" },
    { device_id: "D2", label: "PC-2", path: "D:\\Stuff\\g.exe" },
  ],
  ...over,
});

const unmatched = (n: number): ProposalView =>
  proposal({
    key: `exe:tool${n}.exe|tool ${n}|maker`,
    name: `Tool ${n}`,
    file_name: `tool${n}.exe`,
    product_name: `Tool ${n}`,
    catalog_id: null,
    launcher: "none",
    category: "",
    age_rating: "",
    pc_count: 1,
    devices: [{ device_id: "D1", label: "PC-1", path: `C:\\Tools\\tool${n}.exe` }],
  });

const rowOf = (page: Page, name: string) => page.getByTestId("found-row").filter({ hasText: name });

test("Found on PCs lists matched games with cover and unmatched ones by file name", async ({
  page,
}) => {
  const hub = newHub();
  hub.covers[SHA] = PNG;
  hub.proposals = [proposal({ cover_sha256: SHA }), unmatched(1)];
  await open(page, hub);
  await expect(page.getByTestId("found-row")).toHaveCount(2);

  const great = rowOf(page, "Great Game");
  await expect(great.getByTestId("found-launcher")).toHaveText(en["hub.games.launcher.steam"]);
  await expect(great.getByTestId("found-category")).toHaveText("Shooter");
  await expect(great.getByTestId("found-match")).toHaveText(en["hub.games.found.catalog"]);
  await expect(great.getByTestId("found-pcs")).toHaveText(
    en["hub.games.found.pcs"].replace("{count}", "2"),
  );
  await expect(great.getByTestId("found-where")).toHaveText(
    en["hub.games.found.where"].replace("{names}", "PC-1, PC-2"),
  );
  const cover = great.getByTestId("found-cover");
  await expect(cover).toBeVisible();
  await expect(cover).toHaveAttribute(
    "alt",
    en["hub.games.found.cover_alt"].replace("{name}", "Great Game"),
  );

  const tool = rowOf(page, "Tool 1");
  await expect(tool.getByTestId("found-match")).toHaveText(en["hub.games.found.unmatched"]);
  await expect(tool.getByTestId("found-file")).toHaveText("tool1.exe");
  await expect(tool.getByTestId("found-cover")).toHaveCount(0);
  await expectNoSeriousAxe(page);
});

test("a manager approves several proposals with one library.upsert each", async ({ page }) => {
  const hub = newHub();
  hub.proposals = [proposal(), unmatched(1), unmatched(2)];
  await open(page, hub);
  const approve = page.getByTestId("found-approve");
  await expect(approve).toBeDisabled();

  await rowOf(page, "Great Game").getByTestId("found-select").check();
  await rowOf(page, "Tool 2").getByTestId("found-select").check();
  await expect(approve).toHaveText(en["hub.games.found.approve"].replace("{count}", "2"));
  await approve.click();
  await expect(page.getByTestId("found-status")).toHaveText(
    en["hub.games.found.approved"].replace("{count}", "2"),
  );

  expect(hub.intents.map((i) => i.payload.kind)).toEqual(["library.upsert", "library.upsert"]);
  expect(hub.intents[0].payload).toMatchObject({
    entry_kind: "game",
    name: "Great Game",
    launcher: "steam",
    executable_path: "C:\\Games\\g.exe",
    category: "Shooter",
    age_rating: "16+",
    enabled: true,
    catalog_id: "G1",
    installs: [
      { device_id: "D1", path: "C:\\Games\\g.exe", match_key: "game:G1" },
      { device_id: "D2", path: "D:\\Stuff\\g.exe", match_key: "game:G1" },
    ],
  });
  expect(hub.intents[1].payload).toMatchObject({
    name: "Tool 2",
    launcher: "none",
    executable_path: "C:\\Tools\\tool2.exe",
    installs: [
      { device_id: "D1", path: "C:\\Tools\\tool2.exe", match_key: "exe:tool2.exe|tool 2|maker" },
    ],
  });
  expect(hub.intents[1].payload.catalog_id).toBeUndefined();
  expect(hub.elevations).toHaveLength(0);
  // The approved ones leave the list; the library shows the new entries.
  await expect(page.getByTestId("found-row")).toHaveCount(1);
  await expect(rowOf(page, "Tool 1")).toBeVisible();
});

test("select all, then dismiss sends library.dismiss with the keys and counts them", async ({
  page,
}) => {
  const hub = newHub();
  hub.proposals = [proposal(), unmatched(1)];
  await open(page, hub);
  await page.getByTestId("found-select-all").check();
  await page.getByTestId("found-dismiss").click();
  await expect(page.getByTestId("found-status")).toHaveText(en["hub.games.found.dismissed_done"]);
  expect(hub.intents[0].payload).toEqual({
    kind: "library.dismiss",
    keys: ["game:G1", "exe:tool1.exe|tool 1|maker"],
  });
  await expect(page.getByTestId("found-empty")).toHaveText(en["hub.games.found.empty"]);
  await expect(page.getByTestId("found-dismissed")).toHaveText(
    en["hub.games.found.dismissed"].replace("{count}", "2"),
  );

  await page.getByTestId("found-clear").click();
  await expect(page.getByTestId("found-status")).toHaveText(en["hub.games.found.cleared"]);
  expect(hub.intents[1].payload).toEqual({ kind: "library.dismiss_clear" });
  await expect(page.getByTestId("found-dismissed")).toHaveCount(0);
});

test("a Counter's approval asks a manager's PIN, and again after every ten", async ({ page }) => {
  const hub = newHub();
  hub.proposals = Array.from({ length: 12 }, (_, i) => unmatched(i + 1));
  await open(page, hub, "counter-token");
  await page.getByTestId("found-select-all").check();
  await page.getByTestId("found-approve").click();
  await expect(page.getByTestId("found-approval")).toBeVisible();
  expect(hub.intents).toHaveLength(0);
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");

  // Ten went through on the first PIN; the other two ask again.
  await expect(page.getByTestId("found-status")).toHaveText(
    en["hub.games.found.more"].replace("{count}", "2"),
  );
  expect(hub.elevations[0].items).toEqual(Array(10).fill({ intent_kind: "library.upsert" }));
  expect(hub.intents).toHaveLength(10);
  expect(hub.intents.every((i) => i.ticket === "T1")).toBe(true);
  await expect(page.getByTestId("found-approval")).toBeVisible();
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("found-status")).toHaveText(
    en["hub.games.found.approved"].replace("{count}", "12"),
  );
  expect(hub.elevations[1].items).toEqual(Array(2).fill({ intent_kind: "library.upsert" }));
  expect(hub.intents).toHaveLength(12);
  await expect(page.getByTestId("found-empty")).toBeVisible();
});

test("a Counter's dismissal needs a PIN too", async ({ page }) => {
  const hub = newHub();
  hub.proposals = [unmatched(1)];
  await open(page, hub, "counter-token");
  await rowOf(page, "Tool 1").getByTestId("found-select").check();
  await page.getByTestId("found-dismiss").click();
  await expect(page.getByTestId("found-approval")).toBeVisible();
  await page.keyboard.type("2222");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("found-status")).toHaveText(en["hub.games.found.dismissed_done"]);
  expect(hub.elevations.at(-1)?.items).toEqual([{ intent_kind: "library.dismiss" }]);
  expect(hub.intents[0].ticket).toBe("T1");
});

test("a proposal the Hub refuses is reported and the others still go through", async ({ page }) => {
  const hub = newHub();
  hub.refuseCatalog = "G1";
  hub.proposals = [proposal(), unmatched(1)];
  await open(page, hub);
  await page.getByTestId("found-select-all").check();
  await page.getByTestId("found-approve").click();
  await expect(page.getByTestId("found-status")).toContainText(
    en["hub.games.found.partial"].replace("{done}", "1").replace("{failed}", "1"),
  );
  expect(hub.intents).toHaveLength(2);
  await expect(page.getByTestId("found-row")).toHaveCount(1);
  await expect(rowOf(page, "Great Game")).toBeVisible();
});

test("the found list reloads when the Hub pushes library.changed", async ({ page }) => {
  const hub = newHub();
  await open(page, hub);
  await expect(page.getByTestId("found-empty")).toBeVisible();
  hub.proposals = [unmatched(1)];
  hub.push.push("library.changed");
  await expect(page.getByTestId("found-row")).toHaveCount(1, { timeout: 10_000 });
});

test("a found list that cannot be loaded says so", async ({ page }) => {
  const hub = newHub();
  hub.proposalsDown = true;
  await open(page, hub);
  await expect(page.getByText(en["hub.games.found.load_failed"])).toBeVisible();
});
