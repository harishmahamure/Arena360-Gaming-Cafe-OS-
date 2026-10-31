import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { MEDIA_ORIGIN, mediaUrl, routeOf } from "../apps/marketing/src/lib/site.ts";
import {
  HARDWARE_CATEGORIES,
  assertHelp,
  brandsOf,
  brokenLinks,
  filterHardware,
  isTested,
  parseHardware,
  searchHelp,
  searchIndex,
} from "../apps/marketing/src/lib/help.ts";
import { PRICING_SNAPSHOT } from "../packages/ui/src/pricing.snapshot.ts";
import { TIERS_SNAPSHOT } from "../packages/ui/src/tiers.snapshot.ts";
import { messages } from "../packages/ui/src/messages/index.js";
import { shouldPlay } from "../packages/ui/src/hero-video.js";
import { resolveSiteTheme } from "../packages/ui/src/theme.js";
import { failures, median } from "../apps/marketing/scripts/lighthouse.mjs";

test("High contrast is its own switch, on top of System · Light · Dark", () => {
  assert.equal(resolveSiteTheme("system", false, null, false), "light");
  assert.equal(resolveSiteTheme("system", true, null, false), "dark");
  assert.equal(resolveSiteTheme("light", true, null, false), "light");
  assert.equal(resolveSiteTheme("dark", false, null, false), "dark");
  assert.equal(resolveSiteTheme("dark", false, true, false), "hc");
  assert.equal(resolveSiteTheme("system", true, true, true), "hc");
});

test("prefers-contrast: more defaults High contrast on until the visitor chooses", () => {
  assert.equal(resolveSiteTheme("light", false, null, true), "hc");
  assert.equal(resolveSiteTheme("light", false, false, true), "light");
  assert.equal(resolveSiteTheme("dark", true, false, true), "dark");
  assert.equal(resolveSiteTheme("light", false, true, false), "hc");
});

test("a stored hc from the shared theme key counts as High contrast on", () => {
  assert.equal(resolveSiteTheme("hc", false, null, false), "hc");
  assert.equal(resolveSiteTheme("hc", true, true, false), "hc");
});

test("choosing High contrast off beats a stored hc, and the colour mode takes over", () => {
  assert.equal(resolveSiteTheme("hc", true, false, false), "dark");
  assert.equal(resolveSiteTheme("hc", false, false, true), "light");
});

test("routeOf maps page files to public paths and skips the 404 and dynamic routes", () => {
  assert.equal(routeOf("src/pages/index.astro"), "/");
  assert.equal(routeOf("src/pages/pricing.astro"), "/pricing");
  assert.equal(routeOf("src/pages/pricing/index.astro"), "/pricing");
  assert.equal(routeOf("src/pages/legal/terms.astro"), "/legal/terms");
  assert.equal(routeOf("pages/help/index.astro"), "/help");
  assert.equal(routeOf("src/pages/404.astro"), null);
  assert.equal(routeOf("src/pages/_partial.astro"), null);
  assert.equal(routeOf("src/pages/_x/foo.astro"), null);
  assert.equal(routeOf("src/pages/help/[slug].astro"), null);
  assert.equal(routeOf("src/pages/robots.txt.ts"), null);
});

test("a staging build points at the staging origin, asks crawlers to stay away and hands off to staging", () => {
  const root = fileURLToPath(new URL("../apps/marketing", import.meta.url));
  const out = mkdtempSync(join(tmpdir(), "marketing-staging-"));
  try {
    execFileSync("pnpm", ["exec", "astro", "build", "--mode", "staging", "--outDir", out], {
      cwd: root,
      stdio: "pipe",
      shell: process.platform === "win32",
    });
    const home = readFileSync(join(out, "index.html"), "utf8");
    assert.match(home, /<meta name="robots" content="noindex, nofollow"/);
    assert.match(home, /<link rel="canonical" href="https:\/\/staging\.arena360\.cloud\/"/);
    assert.match(home, /href="https:\/\/start\.staging\.arena360\.cloud"/);
    assert.equal(readFileSync(join(out, "robots.txt"), "utf8"), "User-agent: *\nDisallow: /\n");
    assert.match(
      readFileSync(join(out, "sitemap.xml"), "utf8"),
      /https:\/\/staging\.arena360\.cloud\//,
    );
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("the hero video plays only when it is visible, allowed, not paused and not failed", () => {
  const ok = { reducedMotion: false, visible: true, paused: false, failed: false };
  assert.equal(shouldPlay(ok), true);
  assert.equal(shouldPlay({ ...ok, reducedMotion: true }), false);
  assert.equal(shouldPlay({ ...ok, visible: false }), false);
  assert.equal(shouldPlay({ ...ok, paused: true }), false);
  assert.equal(shouldPlay({ ...ok, failed: true }), false);
});

test("the Lighthouse gate fails on LCP of 2.5 s or more and on scores under 90", () => {
  const pass = { lcp: 2499, performance: 90, accessibility: 90 };
  assert.deepEqual(failures(pass), []);
  assert.equal(failures({ ...pass, lcp: 2500 }).length, 1);
  assert.equal(failures({ ...pass, performance: 89 }).length, 1);
  assert.equal(failures({ ...pass, accessibility: 89 }).length, 1);
  assert.equal(
    failures({ lcp: Number.NaN, performance: Number.NaN, accessibility: 100 }).length,
    2,
  );
  assert.equal(median([3000, 1000, 2000]), 2000);
});

test("every amount on Pricing is a price-book amount and none is typed into the copy", () => {
  const rupees = (minor) => "₹" + new Intl.NumberFormat("en-IN").format(minor / 100);
  const book = new Set(PRICING_SNAPSHOT.bands.IN.lines.map((l) => rupees(l.amount_minor)));

  // The catalogue holds words and placeholders only: a digit next to a rupee sign is a typed price.
  for (const [key, text] of Object.entries(messages)) {
    if (key.startsWith("site.pricing.") || key.startsWith("site.home.gets.")) {
      assert.doesNotMatch(text, /₹\s*\d/, `${key} has a typed amount`);
    }
  }

  const root = fileURLToPath(new URL("../apps/marketing", import.meta.url));
  const out = mkdtempSync(join(tmpdir(), "marketing-pricing-"));
  try {
    execFileSync("pnpm", ["exec", "astro", "build", "--outDir", out], {
      cwd: root,
      stdio: "pipe",
      shell: process.platform === "win32",
    });
    const page = readFileSync(join(out, "pricing", "index.html"), "utf8");
    const main = page.slice(page.indexOf("<main"), page.indexOf("</main>"));
    const shown = new Set(main.match(/₹\d[\d,]*/g));
    assert.ok(
      shown.size >= 6,
      `Pricing shows Free, both paid plans in two cycles and the add-ons, not ${[...shown]}`,
    );
    for (const amount of shown) {
      assert.ok(book.has(amount) || amount === "₹0", `${amount} is not in the price-book snapshot`);
    }
    for (const line of PRICING_SNAPSHOT.bands.IN.lines.filter(
      (l) => l.cycle === "monthly" || /^(basic|premium)$/.test(l.item),
    )) {
      assert.ok(shown.has(rupees(line.amount_minor)), `${line.item} ${line.cycle} is not shown`);
    }
    // Each limit in the comparison table is a tier-table limit.
    for (const row of TIERS_SNAPSHOT) {
      assert.ok(main.includes(`>${row.devices}</td>`), `${row.tier} devices`);
    }
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("marketing media is served from the shared marketing/ prefix of the media origin", () => {
  assert.equal(
    mediaUrl("product/hub/poster.jpg"),
    "https://media.arena360.cloud/marketing/product/hub/poster.jpg",
  );
  assert.ok(mediaUrl("x").startsWith(MEDIA_ORIGIN + "/marketing/"));
});

test("Help links between articles are checked: a link to a missing slug fails the build", () => {
  const article = (slug, body = "") => ({
    slug,
    title: slug,
    summary: slug,
    category: "setup",
    body,
  });
  const ok = [
    article("a", "See [b](/help/b#top) and [hardware](/help#hardware-compatibility)."),
    article("b"),
  ];
  assert.deepEqual(brokenLinks(ok), []);
  assert.doesNotThrow(() => assertHelp(ok));
  const bad = [article("a", "See [c](/help/c) and [b](/help/b)."), article("b")];
  assert.deepEqual(brokenLinks(bad), ["a -> /help/c"]);
  assert.throws(() => assertHelp(bad), /missing article: a -> \/help\/c/);
  assert.throws(() => assertHelp([article("Not Stable")]), /slugs must be lower-case/);
});

test("Help search matches every typed word in a title or summary, title first", () => {
  const index = [
    {
      slug: "a",
      title: "Set up a printer",
      summary: "Connect a receipt printer.",
      category: "setup",
    },
    {
      slug: "b",
      title: "The printer is offline",
      summary: "Fixes for printing.",
      category: "troubleshooting",
    },
    { slug: "c", title: "Pair a kiosk", summary: "Use a printer-free flow.", category: "setup" },
  ];
  assert.deepEqual(
    searchHelp(index, "printer").map((e) => e.slug),
    ["a", "b", "c"],
  );
  assert.deepEqual(
    searchHelp(index, "OFFLINE printer").map((e) => e.slug),
    ["b"],
  );
  assert.deepEqual(
    searchHelp(index, "print")
      .map((e) => e.slug)
      .sort(),
    ["a", "b", "c"],
  );
  assert.deepEqual(searchHelp(index, "zxqv"), []);
  assert.deepEqual(searchHelp(index, "   "), []);
  assert.deepEqual(searchIndex([{ ...index[0], body: "x" }]), [index[0]]);
});

test("the hardware data file is versioned, has a category field and rejects a bad row", () => {
  const row = {
    category: "printer",
    brand: "Northwind",
    model: "NP-80",
    connection: "usb",
    paper_mm: 80,
    result: "works",
    checked: "2026-10-20",
    tested_on: ["windows", "android"],
  };
  const file = parseHardware({
    version: 1,
    rows: [row, { ...row, category: "drawer", paper_mm: null, tested_on: ["windows"] }],
  });
  assert.equal(file.rows.length, 2);
  assert.equal(isTested(file.rows[0]), true);
  assert.equal(isTested(file.rows[1]), false);
  assert.deepEqual(brandsOf(file.rows), ["Northwind"]);
  assert.equal(filterHardware(file.rows, "Northwind", "usb").length, 2);
  assert.equal(filterHardware(file.rows, "Northwind", "lan").length, 0);
  assert.equal(filterHardware(file.rows, "", "").length, 2);
  assert.throws(() => parseHardware({ version: 2, rows: [] }), /version/);
  assert.throws(
    () => parseHardware({ version: 1, rows: [{ ...row, category: "tv" }] }),
    /category/,
  );
  assert.throws(
    () => parseHardware({ version: 1, rows: [{ ...row, paper_mm: null }] }),
    /paper_mm/,
  );
  assert.throws(
    () => parseHardware({ version: 1, rows: [{ ...row, connection: "hdmi" }] }),
    /connection/,
  );
  assert.throws(
    () => parseHardware({ version: 1, rows: [{ ...row, checked: "yesterday" }] }),
    /checked/,
  );
  assert.throws(() => parseHardware({ version: 1, rows: [row, { ...row }] }), /duplicate/);
  assert.deepEqual(HARDWARE_CATEGORIES, ["printer", "drawer"]);
  const shipped = JSON.parse(
    readFileSync(new URL("../apps/marketing/src/data/hardware.json", import.meta.url), "utf8"),
  );
  assert.doesNotThrow(() => parseHardware(shipped));
});
