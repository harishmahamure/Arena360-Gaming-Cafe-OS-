import { test } from "node:test";
import assert from "node:assert/strict";
import {
  EXCLUDED,
  checkContrast,
  contrast,
  derivePairs,
  formatRatio,
  isLargeText,
  luminance,
} from "./check-contrast.mjs";

const ramp = (fontSize, fontWeight = "400") => ({
  fontFamily: "Sora",
  fontSize,
  fontWeight,
  lineHeight: "1",
});

test("WCAG 2.x luminance and ratio", () => {
  assert.equal(luminance("#000000"), 0);
  assert.equal(luminance("#FFF"), 1);
  assert.equal(contrast("#000000", "#FFFFFF"), 21);
  assert.equal(contrast("#FFFFFF", "#000000"), 21);
  assert.equal(contrast("#777777", "#777777"), 1);
  // DESIGN.md's own annotation: state-paused 4.7 on hub-tile
  assert.equal(contrast("#5C82BD", "#141030").toFixed(1), "4.7");
});

test("large text: ≥24px, or ≥18.66px at weight ≥700", () => {
  assert.equal(isLargeText(ramp("24px")), true);
  assert.equal(isLargeText(ramp("23px")), false);
  assert.equal(isLargeText(ramp("19px", "700")), true);
  assert.equal(isLargeText(ramp("19px", "600")), false);
  assert.equal(isLargeText(undefined), false);
});

test("low contrast: text #777777 on bg #666666 fails", () => {
  const fm = { colors: { bg: "#666666", text: "#777777" } };
  assert.deepEqual(checkContrast(fm), ["dark text on bg 1.3 < 4.5"]);
});

test("large text: a 24px component at ~3.2 passes on the 3.0 floor; small text fails", () => {
  const fg = "#707070"; // 3.2 on #222222
  const bg = "#222222";
  const ratio = contrast(fg, bg);
  assert.equal(ratio.toFixed(1), "3.2");
  const fm = (size) => ({
    colors: { "btn-bg": bg, "btn-fg": fg },
    typography: { big: ramp(size) },
    components: {
      cta: {
        background: "{colors.btn-bg}",
        foreground: "{colors.btn-fg}",
        typography: "{typography.big}",
      },
    },
  });
  assert.deepEqual(checkContrast(fm("24px")), []);
  assert.deepEqual(checkContrast(fm("16px")), [`dark btn-fg on btn-bg ${ratio.toFixed(1)} < 4.5`]);
});

test("pairs are derived per theme from twins, ink × surfaces, on-X and chips", () => {
  const fm = {
    colors: {
      bg: "#000000",
      surface: "#000000",
      "hub-tile": "#000000",
      text: "#FFFFFF",
      "state-free": "#00FF00",
      accent: "#FF00FF",
      "on-accent": "#000000",
      "chip-x-bg": "#000000",
      "chip-x-text": "#FFFFFF",
      "border-strong": "#FFFFFF",
      "focus-ring-inner": "#FF00FF",
      "focus-ring-outer": "#FFFFFF",
      "bg-hc": "#000000",
      "text-hc": "#FFFFFF",
      "focus-ring-hc": "#FFFF00",
      "bg-light": "#FFFFFF",
      "text-light": "#000000",
      "accent-text-light": "#880044",
      "group-teal": "#00CCCC",
      "hub-tile-hc": "#000000",
      line: "#111111",
    },
  };
  const keys = derivePairs(fm).map((p) => `${p.theme} ${p.fgName} on ${p.bgName} ${p.floor}`);
  for (const k of [
    "dark text on bg 4.5",
    "dark state-free on hub-tile 4.5",
    "dark on-accent on accent 4.5",
    "dark chip-x-text on chip-x-bg 4.5",
    "dark border-strong on surface 3",
    "dark accent on bg 3",
    "dark focus-ring-inner on bg 3",
    "dark focus-ring-outer on bg 3",
    "dark group-teal on hub-tile 3",
    "hc group-teal on hub-tile-hc 3",
    "hc text-hc on bg-hc 4.5",
    "hc focus-ring-hc on bg-hc 3",
    "light text-light on bg-light 4.5",
    "light accent-text-light on bg-light 4.5",
  ]) {
    assert.ok(keys.includes(k), `missing pair ${k}`);
  }
  // accent-text exists only in light; hub surfaces are not light surfaces
  assert.ok(!keys.some((k) => k.startsWith("dark accent-text")));
  assert.ok(!keys.some((k) => k.startsWith("light") && k.includes("on hub-tile")));
  // a pair that falls back to the dark colours in another theme is reported once
  assert.equal(keys.filter((k) => k.endsWith("on-accent on accent 4.5")).length, 1);
  // decorative hairlines are never checked
  assert.ok(!keys.some((k) => /\bline\b/.test(k)));
});

test("excluded tokens are named with a DESIGN.md reason", () => {
  for (const name of [
    "line",
    "line-hc",
    "hub-line",
    "hub-line-light",
    "warning-light",
    "scrim-base",
    "focus-ring-outer",
  ]) {
    assert.ok(
      EXCLUDED.some((e) => e.match.test(name)),
      name,
    );
  }
  for (const e of EXCLUDED) assert.ok(e.reason.length > 10);
});

test("ratio display never rounds up to the floor", () => {
  assert.equal(formatRatio(1.29, 4.5), "1.3");
  assert.equal(formatRatio(4.47, 4.5), "4.47");
  assert.equal(formatRatio(4.499, 4.5), "4.49");
});

test("decision: dark/hc state inks only on hub-tile, hub-bg and surface; light keeps its surfaces", () => {
  const fm = {
    colors: {
      bg: "#000000",
      surface: "#000000",
      "surface-2": "#5A5A5A",
      "hub-bg": "#000000",
      "hub-header": "#5A5A5A",
      "hub-tile": "#000000",
      text: "#FFFFFF",
      "state-paused": "#7A7A7A",
      "bg-hc": "#000000",
      "surface-hc": "#000000",
      "surface-2-hc": "#5A5A5A",
      "hub-bg-hc": "#000000",
      "hub-header-hc": "#5A5A5A",
      "hub-tile-hc": "#000000",
      "text-hc": "#FFFFFF",
      "state-paused-hc": "#7A7A7A",
      "bg-light": "#FFFFFF",
      "surface-light": "#FFFFFF",
      "surface-2-light": "#6A6A6A",
      "text-light": "#000000",
      "state-paused-light": "#000000",
    },
  };
  const keys = derivePairs(fm).map((p) => `${p.theme} ${p.fg} on ${p.bg}`);
  for (const theme of ["dark", "hc"]) {
    for (const bg of ["hub-tile", "hub-bg", "surface"]) {
      assert.ok(keys.includes(`${theme} state-paused on ${bg}`), `${theme} ${bg}`);
    }
    for (const bg of ["bg", "surface-2", "hub-header"]) {
      assert.ok(!keys.includes(`${theme} state-paused on ${bg}`), `${theme} ${bg}`);
    }
  }
  for (const bg of ["bg", "surface", "surface-2"])
    assert.ok(keys.includes(`light state-paused on ${bg}`));
  // dark/hc state-paused on surface-2 (1.6) is not a drawn pair; light still fails on its surface-2
  assert.deepEqual(
    checkContrast(fm).filter((l) => l.includes("state-paused")),
    ["light state-paused on surface-2 3.9 < 4.5"],
  );
});

test("decision: light skips hub-* colours and Hub-only components", () => {
  const fm = {
    colors: {
      bg: "#000000",
      "hub-tile": "#141030",
      "hub-tile-hc": "#000000",
      "state-order-pending": "#FFBE3D",
      "state-order-pending-hc": "#FFB000",
      "state-order-pending-light": "#8F5300",
      "on-hub-tile": "#FFFFFF",
      "bg-light": "#FFFFFF",
    },
    components: {
      "tile-order-badge": {
        background: "{colors.state-order-pending}",
        foreground: "{colors.hub-tile}",
      },
    },
  };
  const keys = derivePairs(fm).map((p) => `${p.theme} ${p.fg} on ${p.bg}`);
  assert.ok(keys.includes("dark hub-tile on state-order-pending"));
  assert.ok(keys.includes("hc hub-tile on state-order-pending"));
  assert.ok(!keys.some((k) => k.startsWith("light") && k.includes("hub-")), keys.join("\n"));
  assert.deepEqual(checkContrast(fm), []);
});

test("fill-only exclusions apply to ink on surfaces only, not on-X/X or component pairs", () => {
  const fm = {
    colors: {
      bg: "#000000",
      warning: "#FFBE3D",
      "on-warning": "#1E1300",
      "bg-light": "#FFFFFF",
      "warning-light": "#FFBE3D",
      "on-warning-light": "#FFFFFF",
    },
    components: {
      "button-warning": { background: "{colors.warning}", foreground: "{colors.on-warning}" },
    },
  };
  const keys = derivePairs(fm).map((p) => `${p.theme} ${p.fgName} on ${p.bgName}`);
  assert.ok(keys.includes("light on-warning-light on warning-light"));
  // white on #FFBE3D fails: the fill-only exclusion must not hide it
  assert.deepEqual(checkContrast(fm), [
    `light on-warning on warning ${contrast("#FFFFFF", "#FFBE3D").toFixed(1)} < 4.5`,
  ]);
});
