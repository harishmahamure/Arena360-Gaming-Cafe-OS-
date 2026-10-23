#!/usr/bin/env node
// Contrast gate: WCAG 2.x contrast ratios for colour pairs derived automatically from the
// DESIGN.md frontmatter, in the dark, hc and light themes. Fails on any pair below its floor.
// Reads local files only (no network).

import { realpathSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  isHubOnlyComponent,
  readDesign,
  SUFFIX,
  THEMES,
  themeOf,
  themedName,
} from "./gen-tokens.mjs";

export const TEXT_FLOOR = 4.5;
export const LARGE_TEXT_FLOOR = 3.0;
export const NON_TEXT_FLOOR = 3.0;

export const INK = ["text", "text-muted", "ok", "danger", "info", "accent-text", "warning-text"];
export const SURFACES = {
  dark: ["bg", "surface", "surface-2", "hub-bg", "hub-header", "hub-tile"],
  hc: ["bg", "surface", "surface-2", "hub-bg", "hub-header", "hub-tile"],
  light: ["bg", "surface", "surface-2"],
};
/** Where floor-state inks are drawn in dark and hc (human decision); light keeps SURFACES.light. */
export const STATE_SURFACES = {
  dark: ["hub-tile", "hub-bg", "surface"],
  hc: ["hub-tile", "hub-bg", "surface"],
  light: SURFACES.light,
};
export const FOCUS_RINGS = {
  dark: ["focus-ring-inner", "focus-ring-outer"],
  hc: ["focus-ring"],
  light: ["focus-ring"],
};
export const GROUP_SWATCH_SURFACES = ["hub-tile", "hub-tile-hc"];

/**
 * Tokens documented in DESIGN.md as fill-only or decorative; never checked as ink or boundary.
 * `match` is tested against the colour name (theme suffix included).
 */
export const EXCLUDED = [
  {
    match: /^line(-|$)/,
    reason: "line*: decorative hairline only; never an input or focusable boundary",
  },
  { match: /^hub-line(-|$)/, reason: "hub-line*: decorative hairline only" },
  {
    match: /^warning-light$/,
    reason: "warning-light: fill only (1.5 on bg, so always edged; see button-warning)",
  },
  {
    match: /^scrim-base$/,
    reason: "scrim-base: the rgba scrim gradient base, never solid ink or fill",
  },
  {
    match: /^focus-ring-outer$/,
    over: "media",
    reason:
      "focus-ring-outer over media: carries the ring over dark video/art, which has no token to measure",
  },
];

const isExcluded = (name) => EXCLUDED.some((e) => !e.over && e.match.test(name));

/** `#rgb` / `#rrggbb` -> [r, g, b] in 0..255. */
export function parseHex(hex) {
  const m = String(hex).match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (!m) throw new Error(`not a #rgb/#rrggbb colour: ${hex}`);
  const h = m[1].length === 3 ? [...m[1]].map((c) => c + c).join("") : m[1];
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
}

/** WCAG 2.x relative luminance. */
export function luminance(hex) {
  const [r, g, b] = parseHex(hex).map((c) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG 2.x contrast ratio (1..21). */
export function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Whether a typography ramp counts as large text (≥24px, or ≥18.66px at weight ≥700). */
export function isLargeText(ramp) {
  if (!ramp) return false;
  const size = parseFloat(ramp.fontSize);
  const weight = Number(ramp.fontWeight ?? 400);
  return size >= 24 || (size >= 18.66 && weight >= 700);
}

const colorRef = (value) =>
  typeof value === "string" ? value.match(/^\{colors\.([\w-]+)\}$/)?.[1] : undefined;
const typographyRef = (value) =>
  typeof value === "string" ? value.match(/^\{typography\.([\w-]+)\}$/)?.[1] : undefined;

/**
 * Derives every checked pair from frontmatter.
 * Base names (no theme suffix) are resolved to their theme twin, falling back to the dark value
 * as the generated CSS does. Names only present in one theme are checked where they exist.
 * @returns {Array<{theme: string, fg: string, bg: string, fgName: string, bgName: string, floor: number, why: string}>}
 */
export function derivePairs(fm) {
  const colors = fm.colors ?? {};
  const bases = Object.keys(colors).filter((n) => themeOf(n).theme === "dark");
  const pairs = [];
  /** Colour name for logical name `n` in `theme`, or undefined when it has no value there. */
  const resolve = (n, theme) => {
    if (bases.includes(n)) return themedName(colors, n, theme);
    const twin = theme === "dark" ? n : n + SUFFIX[theme];
    return twin in colors ? twin : undefined;
  };
  /** Fill-only/decorative exclusions apply to ink-on-surface pairs only (`exclude`). */
  const add = (theme, fg, bg, floor, why, exclude = false) => {
    const fgName = resolve(fg, theme);
    const bgName = resolve(bg, theme);
    if (!fgName || !bgName) return;
    if (exclude && (isExcluded(fgName) || isExcluded(bgName))) return;
    // The Hub is dark + HC only: Hub-only colours are not checked in light.
    if (theme === "light" && (/^hub-/.test(fg) || /^hub-/.test(bg))) return;
    pairs.push({ theme, fg, bg, fgName, bgName, floor, why });
  };

  for (const theme of THEMES) {
    const surfaces = SURFACES[theme];
    // 1. Components with both a foreground and a background colour.
    for (const [name, props] of Object.entries(fm.components ?? {})) {
      const fg = colorRef(props?.foreground);
      const bg = colorRef(props?.background);
      if (!fg || !bg) continue;
      if (theme === "light" && isHubOnlyComponent(props)) continue;
      const ramp = fm.typography?.[typographyRef(props.typography)];
      add(theme, fg, bg, isLargeText(ramp) ? LARGE_TEXT_FLOOR : TEXT_FLOOR, `component ${name}`);
    }
    // 2. Ink tokens on every surface of the theme.
    for (const fg of INK) for (const bg of surfaces) add(theme, fg, bg, TEXT_FLOOR, "ink", true);
    for (const fg of bases.filter((b) => b.startsWith("state-"))) {
      for (const bg of STATE_SURFACES[theme]) add(theme, fg, bg, TEXT_FLOOR, "state ink", true);
    }
    // 3. on-X on X, chip-*-text on chip-*-bg.
    for (const b of bases) {
      if (b.startsWith("on-")) add(theme, b, b.slice(3), TEXT_FLOOR, "on-colour");
      const chip = b.match(/^(chip-.+)-text$/);
      if (chip) add(theme, b, `${chip[1]}-bg`, TEXT_FLOOR, "chip");
    }
    // 4. Non-text boundaries.
    for (const bg of surfaces) {
      add(theme, "border-strong", bg, NON_TEXT_FLOOR, "boundary");
      add(theme, "accent", bg, NON_TEXT_FLOOR, "accent");
      for (const ring of FOCUS_RINGS[theme]) add(theme, ring, bg, NON_TEXT_FLOOR, "focus ring");
    }
  }
  // 4. Group swatches against the hub tile (dark) and the hc tile, given by exact names.
  for (const swatch of Object.keys(colors).filter((n) => n.startsWith("group-"))) {
    for (const bg of GROUP_SWATCH_SURFACES) {
      if (!(bg in colors)) continue;
      const theme = themeOf(bg).theme;
      pairs.push({
        theme,
        fg: swatch,
        bg,
        fgName: swatch,
        bgName: bg,
        floor: NON_TEXT_FLOOR,
        why: "group swatch",
      });
    }
  }

  // Deduplicate by resolved colour names: a pair that falls back to the same colours as an
  // earlier theme is reported once, under the first theme, with the strictest floor.
  const seen = new Map();
  for (const p of pairs) {
    const key = `${p.fgName}\u0000${p.bgName}`;
    const prev = seen.get(key);
    if (!prev) seen.set(key, p);
    else if (p.floor > prev.floor) seen.set(key, { ...prev, floor: p.floor, why: p.why });
  }
  return [...seen.values()];
}

/** Ratio for display, never rounded up to the floor it failed. */
export function formatRatio(ratio, floor) {
  const one = Math.round(ratio * 10) / 10;
  if (one < floor) return one.toFixed(1);
  return (Math.floor(ratio * 100) / 100).toFixed(2);
}

/** @returns {string[]} one line per failing pair */
export function checkContrast(fm) {
  const colors = fm.colors ?? {};
  const out = [];
  for (const p of derivePairs(fm)) {
    const ratio = contrast(colors[p.fgName], colors[p.bgName]);
    if (ratio < p.floor) {
      out.push(
        `${p.theme} ${p.fg} on ${p.bg} ${formatRatio(ratio, p.floor)} < ${p.floor.toFixed(1)}`,
      );
    }
  }
  return out;
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const fm = readDesign(root);
  const failures = checkContrast(fm);
  if (failures.length > 0) {
    for (const f of failures) console.error(f);
    console.error(`contrast check: ${failures.length} failing pair(s)`);
    process.exit(1);
  }
  console.log(`contrast check: ok (${derivePairs(fm).length} pairs)`);
}

const isMain = (() => {
  try {
    return (
      Boolean(process.argv[1]) &&
      realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1])
    );
  } catch {
    return false;
  }
})();

if (isMain) {
  try {
    main();
  } catch (err) {
    console.error(err.message);
    process.exit(1);
  }
}
