// Frontend lockdown guards, defense in depth behind the Rust keyboard hook and the WebView2
// settings: block the context menu and the browser accelerator keys that would escape the
// kiosk (print, reload, find, devtools, save, new window), and neutralise `window.print` and
// any file picker. These run in the real webview; in a plain browser (dev, Playwright) they are
// harmless.

/** Key combos that must never reach the browser. */
function isBlockedCombo(e: KeyboardEvent): boolean {
  const k = e.key.toLowerCase();
  if (k === "f5" || k === "f12") return true;
  if (e.ctrlKey && e.shiftKey && (k === "i" || k === "j" || k === "c")) return true;
  if (e.ctrlKey && !e.altKey && ["p", "r", "f", "s", "n", "j", "u", "o"].includes(k)) return true;
  return false;
}

/** Installs the guards; returns a cleanup that removes them. */
export function installKioskGuards(): () => void {
  const onContextMenu = (e: Event) => e.preventDefault();
  const onKeyDown = (e: KeyboardEvent) => {
    if (isBlockedCombo(e)) {
      e.preventDefault();
      e.stopPropagation();
    }
  };
  // A file picker would open an Explorer-like dialog that escapes the kiosk.
  const onBeforePrint = (e: Event) => e.preventDefault();

  window.addEventListener("contextmenu", onContextMenu, { capture: true });
  window.addEventListener("keydown", onKeyDown, { capture: true });
  window.addEventListener("beforeprint", onBeforePrint);
  const realPrint = window.print;
  try {
    window.print = () => {};
  } catch {
    // Some engines make print read-only; the beforeprint handler still cancels it.
  }

  return () => {
    window.removeEventListener("contextmenu", onContextMenu, { capture: true });
    window.removeEventListener("keydown", onKeyDown, { capture: true });
    window.removeEventListener("beforeprint", onBeforePrint);
    try {
      window.print = realPrint;
    } catch {
      // ignore
    }
  };
}

/** The primary monitor box the Rust shell injected (physical px relative to the window). */
type PrimaryBox = { x: number; y: number; width: number; height: number };

function readPrimary(): PrimaryBox | null {
  const b = (window as unknown as { __ARENA_PRIMARY__?: PrimaryBox }).__ARENA_PRIMARY__;
  return b && typeof b.width === "number" && b.width > 0 ? b : null;
}

/**
 * Positions `el` over the primary monitor (so the locked placeholder is not stretched across
 * every screen). Converts the shell's physical px to CSS px with the device pixel ratio. When
 * no box was injected (dev, a single screen), `el` keeps its full-window position. Returns a
 * cleanup for the display-change listener.
 */
export function placeOnPrimary(el: HTMLElement): () => void {
  const apply = () => {
    const b = readPrimary();
    if (!b) {
      el.style.removeProperty("left");
      el.style.removeProperty("top");
      el.style.removeProperty("width");
      el.style.removeProperty("height");
      return;
    }
    const dpr = window.devicePixelRatio || 1;
    const px = (n: number) => `${n / dpr}px`; // i18n-ignore (CSS length, not prose)
    el.style.left = px(b.x);
    el.style.top = px(b.y);
    el.style.width = px(b.width);
    el.style.height = px(b.height);
  };
  apply();
  window.addEventListener("arena-primary", apply);
  return () => window.removeEventListener("arena-primary", apply);
}
