import { SESSION_DEPENDENCY, loadSession } from "../lib/auth";
import { SNAPSHOT_DEPENDENCY, loadSnapshot } from "../lib/snapshot";
import { applyTheme } from "../lib/ui-settings.svelte";

// The Hub UI is a client-only SPA inside Tauri (adapter-static with an index.html fallback).
export const ssr = false;

/**
 * The branch snapshot and the signed-in staff member are read before the first render, so the
 * floor never shows a skeleton and the sign-in gate never flashes a screen. An unreachable Hub
 * resolves to `{ ok: false }` and the layout shows the error state.
 * @type {import('./$types').LayoutLoad}
 */
export async function load({ fetch, depends }) {
  depends(SNAPSHOT_DEPENDENCY);
  depends(SESSION_DEPENDENCY);
  const [hub, session] = await Promise.all([loadSnapshot(fetch), loadSession(fetch)]);
  // The theme is on <html> before the shell renders (no dark flash on an HC Hub).
  if (hub.ok) applyTheme(hub.snapshot.ui.hc);
  return { hub, session };
}
