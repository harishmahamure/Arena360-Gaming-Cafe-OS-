import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { routeOf } from "../apps/marketing/src/lib/site.ts";
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
