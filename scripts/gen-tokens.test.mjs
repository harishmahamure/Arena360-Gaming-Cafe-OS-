import { test } from "node:test";
import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  CSS_HEADER,
  DESIGN_MD,
  TS_HEADER,
  componentColorTokens,
  componentCompoundTokens,
  generate,
  hcTwin,
  hubOnlyComponents,
  isHubOnlyComponent,
  parseFrontmatter,
  readDesign,
  renderCss,
  renderTokens,
  themeOf,
  validate,
} from "./gen-tokens.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const FM = {
  colors: {
    bg: "#000000",
    text: "#FFFFFF",
    "hub-tile": "#111111",
    "bg-hc": "#000000",
    "text-hc": "#FFFFFF",
    "hub-tile-hc": "#000000",
    "bg-light": "#FAFAFA",
    "text-light": "#111111",
    "accent-text-light": "#AA0000",
  },
  typography: {
    "hub-micro": {
      fontFamily: "Sora",
      fontSize: "11px",
      fontWeight: "700",
      lineHeight: "1.2",
      letterSpacing: "0.06em",
      note: "Uppercase",
    },
    "hub-numeric": {
      fontFamily: "Sora",
      fontSize: "26px",
      fontWeight: "800",
      lineHeight: "1.1",
      note: "Tabular numerals (tnum)",
    },
  },
  rounded: { sm: "10px" },
  spacing: { 1: "4px", "touch-min": "44px" },
  elevation: { menu: "0 16px 40px rgba(0,0,0,.35)", none: "none" },
  components: {
    "hud-pill": {
      background: "{colors.bg}",
      foreground: "{colors.text}",
      typography: "{typography.hub-numeric}",
      subtle: "1px solid {colors.text}",
    },
    tile: { background: "{colors.hub-tile}", activeMarker: "{colors.text}" },
  },
};

test("frontmatter parsing", () => {
  assert.deepEqual(parseFrontmatter("---\na: 1\nb: x\n---\n# Doc\n"), { a: 1, b: "x" });
  assert.throws(() => parseFrontmatter("# no frontmatter\n"), /no YAML frontmatter/);
});

test("theme suffixes", () => {
  assert.deepEqual(themeOf("bg"), { theme: "dark", base: "bg" });
  assert.deepEqual(themeOf("surface-2-hc"), { theme: "hc", base: "surface-2" });
  assert.deepEqual(themeOf("warning-light"), { theme: "light", base: "warning" });
});

test("css declares every colour, theme aliases with light fallback, and all scales", () => {
  const { css, names } = renderCss(FM);
  assert.ok(css.startsWith(CSS_HEADER));
  for (const n of Object.keys(FM.colors))
    assert.ok(css.includes(`--color-${n}: ${FM.colors[n]};`), n);
  // dark default + explicit dark theme
  assert.match(css, /:root,\n\[data-theme="dark"\] \{\n {2}--bg: var\(--color-bg\);/);
  assert.match(css, /\[data-theme="hc"\] \{[^}]*--hub-tile: var\(--color-hub-tile-hc\);/);
  // light twin used where it exists, dark value where DESIGN.md has none
  assert.match(css, /\[data-theme="light"\] \{[^}]*--text: var\(--color-text-light\);/);
  assert.match(css, /\[data-theme="light"\] \{[^}]*--hub-tile: var\(--color-hub-tile\);/);
  assert.ok(css.includes("--space-touch-min: 44px;"));
  assert.ok(css.includes("--space-1: 4px;"));
  assert.ok(css.includes("--radius-sm: 10px;"));
  assert.ok(css.includes("--elevation-menu: 0 16px 40px rgba(0,0,0,.35);"));
  assert.ok(css.includes("--elevation-none: none;"));
  assert.ok(css.includes('--type-hub-micro-family: "Sora Variable", "Sora";'));
  assert.ok(css.includes("--type-hub-micro-letter-spacing: 0.06em;"));
  assert.ok(css.includes("--type-hub-numeric-letter-spacing: normal;"));
  assert.match(css, /\.type-hub-micro \{[^}]*text-transform: uppercase;/);
  assert.match(css, /\.type-hub-numeric \{[^}]*font-variant-numeric: tabular-nums;/);
  assert.match(css, /\.tabular-nums \{\n {2}font-variant-numeric: tabular-nums;\n\}/);
  assert.ok(names.includes("--hud-pill-background"));
});

test("component tokens: single colour refs, kebab-cased, with -hc twins", () => {
  assert.deepEqual(
    componentColorTokens(FM).map((c) => c.name),
    ["--hud-pill-background", "--hud-pill-foreground", "--tile-background", "--tile-active-marker"],
  );
  const { css } = renderCss(FM);
  assert.ok(css.includes("--tile-active-marker: var(--text);"));
  assert.ok(css.includes("--tile-active-marker-hc: var(--color-text-hc);"));
  assert.ok(css.includes("--hud-pill-subtle: 1px solid var(--color-text);"));
  assert.ok(css.includes("--hud-pill-subtle-hc: 1px solid var(--color-text-hc);"));
});

test("missing twin: a component colour without -hc fails generation naming it", () => {
  const fm = {
    ...FM,
    colors: { ...FM.colors, foo: "#123456" },
    components: { x: { background: "{colors.foo}" } },
  };
  assert.deepEqual(validate(fm), [
    "missing high-contrast twin: components.x.background references foo, but colors has no foo-hc",
  ]);
  assert.throws(() => renderTokens(fm), /colors has no foo-hc/);
});

test("unresolved references fail generation", () => {
  const fm = {
    ...FM,
    components: { x: { border: "1px solid {colors.nope}", t: "{typography.gone}" } },
  };
  assert.deepEqual(validate(fm), [
    "unresolved reference {colors.nope} in components.x.border",
    "unresolved reference {typography.gone} in components.x.t",
  ]);
  assert.throws(() => renderTokens(fm), /\{colors\.nope\}/);
});

test("ts output has typed constants and the TokenName union", () => {
  const ts = renderTokens(FM)["tokens.ts"];
  assert.ok(ts.startsWith(TS_HEADER));
  for (const name of ["colors", "typography", "spacing", "radius", "elevation", "components"]) {
    assert.match(ts, new RegExp(`export const ${name} = \\{[\\s\\S]*?\\} as const;`));
  }
  assert.ok(ts.includes('  | "--color-bg"'));
  assert.ok(ts.includes('  | "--tile-active-marker-hc"'));
});

test("the real DESIGN.md generates, with elevation and every component twin", () => {
  assert.ok(DESIGN_MD.endsWith("DESIGN.md"));
  const fm = readDesign(root);
  assert.deepEqual(validate(fm), []);
  assert.deepEqual(Object.keys(fm.elevation), ["game-tile", "menu", "toast", "none"]);
  const { css } = renderCss(fm);
  assert.ok(css.includes("--space-touch-min: 44px;"));
  // committed output matches (modulo prettier formatting) in its variable set
  const committed = readFileSync(join(root, "packages/ui/src/tokens/tokens.css"), "utf8");
  for (const m of css.matchAll(/^ {2}(--[\w-]+):/gm))
    assert.ok(committed.includes(`${m[1]}:`), m[1]);
});

test("decision: Hub-only components get no light mapping", () => {
  const fm = {
    ...FM,
    colors: {
      ...FM.colors,
      "state-x": "#00FF00",
      "state-x-hc": "#00FF00",
      "state-x-light": "#006600",
    },
    components: {
      badge: {
        background: "{colors.state-x}",
        foreground: "{colors.hub-tile}",
        border: "1px {colors.hub-tile}",
      },
      mixed: { background: "{colors.hub-tile}", foreground: "{colors.text}" },
    },
  };
  assert.equal(isHubOnlyComponent(fm.components.badge), true);
  assert.equal(isHubOnlyComponent(fm.components.mixed), false);
  assert.equal(isHubOnlyComponent({ radius: "{rounded.sm}" }), false);
  assert.deepEqual(hubOnlyComponents(fm), ["badge"]);
  const { css } = renderCss(fm);
  const block = (sel) => css.slice(css.indexOf(sel)).split("}")[0];
  const shared = block(":root,\n[data-theme] {");
  const hub = block(':root,\n[data-theme="dark"],\n[data-theme="hc"] {');
  assert.ok(hub.includes("--badge-background: var(--state-x);"));
  assert.ok(!shared.includes("--badge-"));
  assert.ok(shared.includes("--mixed-background: var(--hub-tile);"));
  assert.ok(css.includes("--badge-background-hc: var(--color-state-x-hc);"));
  const ts = renderTokens(fm)["tokens.ts"];
  assert.match(ts, /export const hubOnlyComponents = \["badge"\] as const;/);
});

const block = (css, selector) => css.slice(css.indexOf(selector)).split("}")[0];

test("twin-only colours get theme aliases; --focus-ring is themed", () => {
  const fm = {
    ...FM,
    colors: {
      ...FM.colors,
      "focus-ring-inner": "#FF4FA8",
      "focus-ring-hc": "#FFE14D",
      "focus-ring-light": "#AD0E63",
      "warning-text-light": "#8F5300",
    },
  };
  const { css } = renderCss(fm);
  const dark = block(css, ':root,\n[data-theme="dark"] {');
  const hc = block(css, '[data-theme="hc"] {');
  const light = block(css, '[data-theme="light"] {');
  assert.ok(dark.includes("--focus-ring: var(--color-focus-ring-inner);"));
  assert.ok(hc.includes("--focus-ring: var(--color-focus-ring-hc);"));
  assert.ok(light.includes("--focus-ring: var(--color-focus-ring-light);"));
  assert.ok(light.includes("--accent-text: var(--color-accent-text-light);"));
  assert.ok(light.includes("--warning-text: var(--color-warning-text-light);"));
  assert.ok(!dark.includes("--accent-text:"));
});

test("compound component values: every colour ref is twin-checked and emitted with var()", () => {
  const fm = {
    ...FM,
    colors: {
      ...FM.colors,
      "focus-ring-inner": "#FF4FA8",
      "focus-ring-hc": "#FFE14D",
      "group-teal": "#3FD0D4",
    },
    components: {
      field: {
        border: "1px solid {colors.text}",
        focus: "2px solid {colors.focus-ring-inner}, {rounded.sm}",
        dot: "8px circle in {colors.group-teal}",
        borderLight: "1px solid {colors.accent-text-light}",
      },
    },
  };
  assert.deepEqual(validate(fm), []);
  assert.deepEqual(componentCompoundTokens(fm), [
    {
      name: "--field-border",
      value: "1px solid var(--color-text)",
      hc: "1px solid var(--color-text-hc)",
    },
    {
      name: "--field-focus",
      value: "2px solid var(--color-focus-ring-inner), var(--radius-sm)",
      hc: "2px solid var(--color-focus-ring-hc), var(--radius-sm)",
    },
    {
      name: "--field-dot",
      value: "8px circle in var(--color-group-teal)",
      hc: "8px circle in var(--color-group-teal)",
    },
    { name: "--field-border-light", value: "1px solid var(--color-accent-text-light)", hc: null },
  ]);
  const { css } = renderCss(fm);
  assert.ok(css.includes("--field-border: 1px solid var(--color-text);"));
  assert.ok(css.includes("--field-border-hc: 1px solid var(--color-text-hc);"));
  assert.ok(!css.includes("--field-border-light-hc"));
  assert.equal(hcTwin(fm.colors, "text"), "text-hc");
  assert.equal(hcTwin(fm.colors, "focus-ring-hc"), "focus-ring-hc");
  assert.equal(hcTwin({}, "nope"), undefined);
});

test("missing twin inside a compound value fails naming the colour", () => {
  const fm = {
    ...FM,
    colors: { ...FM.colors, foo: "#123456" },
    components: { x: { border: "2px solid {colors.foo}" } },
  };
  assert.deepEqual(validate(fm), [
    "missing high-contrast twin: components.x.border references foo, but colors has no foo-hc",
  ]);
});

test("a component token named like a colour alias is not emitted", () => {
  const fm = {
    ...FM,
    colors: { ...FM.colors, "focus-ring-inner": "#FF4FA8", "focus-ring-hc": "#FFE14D" },
    components: { "focus-ring": { inner: "2px solid {colors.focus-ring-inner}" } },
  };
  const { css } = renderCss(fm);
  assert.ok(!css.includes("--focus-ring-inner: 2px"));
  assert.ok(css.includes("--focus-ring-inner: var(--color-focus-ring-inner);"));
});

test("a type ramp missing a required field fails naming the ramp and field", () => {
  const fm = {
    ...FM,
    typography: {
      ...FM.typography,
      "hub-body": { fontFamily: "Sora", fontSize: "15px", lineHeight: "1.45" },
    },
  };
  assert.deepEqual(validate(fm), ["typography.hub-body is missing fontWeight"]);
  assert.throws(() => renderTokens(fm), /typography\.hub-body is missing fontWeight/);
});

test("duplicate generated names within a block fail", () => {
  const fm = {
    ...FM,
    components: { a: { "b-c": "{colors.text}" }, "a-b": { c: "{colors.text}" } },
  };
  assert.throws(() => renderCss(fm), /duplicate token --a-b-c-hc in :root/);
});

test("generate swaps the folder in and leaves no backup", async () => {
  const root = mkdtempSync(join(tmpdir(), "gen-tokens-"));
  try {
    mkdirSync(join(root, DESIGN_MD, ".."), { recursive: true });
    writeFileSync(join(root, DESIGN_MD), `---\n${JSON.stringify(FM)}\n---\n# Design\n`);
    mkdirSync(join(root, "packages/ui/src/tokens"), { recursive: true });
    writeFileSync(join(root, "packages/ui/src/tokens/old.txt"), "old\n");
    assert.deepEqual(await generate(root), ["tokens.css", "tokens.ts"]);
    assert.deepEqual(readdirSync(join(root, "packages/ui/src/tokens")).sort(), [
      "tokens.css",
      "tokens.ts",
    ]);
    assert.deepEqual(readdirSync(join(root, "packages/ui/src")), ["tokens"]);
    // a failing render leaves the committed folder untouched
    writeFileSync(join(root, DESIGN_MD), "no frontmatter\n");
    await assert.rejects(() => generate(root), /no YAML frontmatter/);
    assert.ok(existsSync(join(root, "packages/ui/src/tokens/tokens.css")));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
