// The marketing site's theme state, shared by every island on the page (footer toggle, phone nav).
// System · Light · Dark is the colour mode; High contrast is a separate switch (see theme.js).
// The inline head script in the site layout paints the same result before first paint.

import {
  readContrastChoice,
  readThemeChoice,
  resolveSiteTheme,
  storeContrastChoice,
  storeThemeChoice,
} from "./theme.js";

/** @typedef {import("./theme.js").ThemeChoice} ThemeChoice */

export const siteTheme = $state({
  /** @type {"system" | "light" | "dark"} */
  mode: "system",
  /** @type {boolean | null} */
  contrast: null,
  prefersDark: false,
  prefersMore: false,
});

function apply() {
  document.documentElement.dataset.theme = resolveSiteTheme(
    siteTheme.mode,
    siteTheme.prefersDark,
    siteTheme.contrast,
    siteTheme.prefersMore,
  );
}

/** @param {"system" | "light" | "dark"} mode */
export function chooseMode(mode) {
  siteTheme.mode = mode;
  storeThemeChoice(mode);
  apply();
}

/** @param {boolean} on */
export function chooseContrast(on) {
  siteTheme.contrast = on;
  storeContrastChoice(on);
  apply();
}

let users = 0;
/** @type {(() => void) | undefined} */
let stop;

/**
 * Loads the stored choices and follows `prefers-color-scheme` and `prefers-contrast` live while
 * at least one island is mounted. Returns the island's cleanup.
 */
export function useSiteTheme() {
  if (users++ === 0) {
    const stored = readThemeChoice();
    siteTheme.mode = stored === "light" || stored === "dark" ? stored : "system";
    siteTheme.contrast = stored === "hc" ? true : readContrastChoice();
    const dark = window.matchMedia?.("(prefers-color-scheme: dark)");
    const more = window.matchMedia?.("(prefers-contrast: more)");
    siteTheme.prefersDark = dark?.matches ?? false;
    siteTheme.prefersMore = more?.matches ?? false;
    apply();
    /** @param {MediaQueryListEvent} e */
    const onDark = (e) => {
      siteTheme.prefersDark = e.matches;
      apply();
    };
    /** @param {MediaQueryListEvent} e */
    const onMore = (e) => {
      siteTheme.prefersMore = e.matches;
      apply();
    };
    dark?.addEventListener("change", onDark);
    more?.addEventListener("change", onMore);
    stop = () => {
      dark?.removeEventListener("change", onDark);
      more?.removeEventListener("change", onMore);
    };
  }
  return () => {
    if (--users === 0) stop?.();
  };
}
