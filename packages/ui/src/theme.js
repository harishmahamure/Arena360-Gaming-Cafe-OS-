// The web surfaces' theme choice (System · Light · Dark), remembered per device. The onboarding
// app's `app.html` repeats `THEME_KEY` and `resolveTheme` in a small inline script, so the
// stored theme is set on `<html>` before first paint.

/** The `localStorage` key holding the choice. */
export const THEME_KEY = "arena.theme";

/** @typedef {"system" | "light" | "dark"} ThemeChoice */

/** @type {ThemeChoice[]} */
export const THEME_CHOICES = ["system", "light", "dark"];

/**
 * The stored choice, or `system` when nothing (or something unreadable) is stored.
 * @returns {ThemeChoice}
 */
export function readThemeChoice() {
  try {
    const v = globalThis.localStorage?.getItem(THEME_KEY);
    return v === "light" || v === "dark" ? v : "system";
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
 * @returns {"light" | "dark"}
 */
export function resolveTheme(choice, prefersDark) {
  if (choice === "light" || choice === "dark") return choice;
  return prefersDark ? "dark" : "light";
}
