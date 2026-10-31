// The web surfaces' theme choice (System · Light · Dark · High contrast), remembered per device. The onboarding
// app's `app.html` repeats `THEME_KEY` and `resolveTheme` in a small inline script, so the
// stored theme is set on `<html>` before first paint.

/** The `localStorage` key holding the choice. */
export const THEME_KEY = "arena.theme";

/** @typedef {"system" | "light" | "dark" | "hc"} ThemeChoice */

/** @type {ThemeChoice[]} */
export const THEME_CHOICES = ["system", "light", "dark", "hc"];

/**
 * The stored choice, or `system` when nothing (or something unreadable) is stored.
 * @returns {ThemeChoice}
 */
export function readThemeChoice() {
  try {
    const v = globalThis.localStorage?.getItem(THEME_KEY);
    return v === "light" || v === "dark" || v === "hc" ? v : "system";
  } catch {
    return "system";
  }
}

/**
 * Remembers `choice` on this device (silently skipped where storage is blocked).
 * @param {ThemeChoice} choice
 */
export function storeThemeChoice(choice) {
  try {
    globalThis.localStorage?.setItem(THEME_KEY, choice);
  } catch {
    // Storage blocked (private mode, disabled site data): the choice lasts for this page only.
  }
}

/**
 * The theme a choice paints: `system` follows `prefers-color-scheme`.
 * @param {ThemeChoice} choice
 * @param {boolean} prefersDark
 * @returns {"light" | "dark" | "hc"}
 */
export function resolveTheme(choice, prefersDark) {
  if (choice === "light" || choice === "dark" || choice === "hc") return choice;
  return prefersDark ? "dark" : "light";
}

// The marketing site keeps High contrast apart from System · Light · Dark: a separate on/off
// switch under its own key. With nothing stored, `prefers-contrast: more` turns it on until the
// visitor chooses. The site's inline head script repeats `CONTRAST_KEY` and `resolveSiteTheme`.

/** The `localStorage` key holding the visitor's High contrast choice (`"on"` or `"off"`). */
export const CONTRAST_KEY = "arena.contrast";

/**
 * The stored High contrast choice: `true` (on), `false` (off) or `null` (never chosen).
 * @returns {boolean | null}
 */
export function readContrastChoice() {
  try {
    const v = globalThis.localStorage?.getItem(CONTRAST_KEY);
    return v === "on" ? true : v === "off" ? false : null;
  } catch {
    return null;
  }
}

/**
 * Remembers the High contrast choice on this device (silently skipped where storage is blocked).
 * @param {boolean} on
 */
export function storeContrastChoice(on) {
  try {
    globalThis.localStorage?.setItem(CONTRAST_KEY, on ? "on" : "off");
  } catch {
    // Storage blocked: the choice lasts for this page only.
  }
}

/**
 * The theme the marketing site paints. High contrast wins over the colour mode; a visitor who
 * never chose it gets it from `prefers-contrast: more`.
 * @param {ThemeChoice} choice  `system`, `light` or `dark` (a stored `hc` counts as High contrast on until the switch is chosen)
 * @param {boolean} prefersDark
 * @param {boolean | null} contrast  the stored High contrast choice
 * @param {boolean} prefersMore  `prefers-contrast: more`
 * @returns {"light" | "dark" | "hc"}
 */
export function resolveSiteTheme(choice, prefersDark, contrast, prefersMore) {
  if (contrast ?? (choice === "hc" || prefersMore)) return "hc";
  return resolveTheme(
    /** @type {ThemeChoice} */ (choice) === "hc" ? "system" : choice,
    prefersDark,
  );
}
