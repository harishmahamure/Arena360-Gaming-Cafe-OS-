import assert from "node:assert/strict";
import { test } from "node:test";
import { routeOf } from "../apps/marketing/src/lib/site.ts";
import { resolveSiteTheme } from "../packages/ui/src/theme.js";

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
  assert.equal(resolveSiteTheme("hc", true, false, false), "hc");
});

test("routeOf maps page files to public paths and skips the 404 and dynamic routes", () => {
  assert.equal(routeOf("src/pages/index.astro"), "/");
  assert.equal(routeOf("src/pages/pricing.astro"), "/pricing");
  assert.equal(routeOf("src/pages/pricing/index.astro"), "/pricing");
  assert.equal(routeOf("src/pages/legal/terms.astro"), "/legal/terms");
  assert.equal(routeOf("pages/help/index.astro"), "/help");
  assert.equal(routeOf("src/pages/404.astro"), null);
  assert.equal(routeOf("src/pages/help/[slug].astro"), null);
  assert.equal(routeOf("src/pages/robots.txt.ts"), null);
});
