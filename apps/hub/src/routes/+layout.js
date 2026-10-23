import { SNAPSHOT_DEPENDENCY, loadSnapshot } from "../lib/snapshot";

// The Hub UI is a client-only SPA inside Tauri (adapter-static with an index.html fallback).
export const ssr = false;

/**
 * The branch snapshot is read before the first render, so the floor never shows a skeleton.
 * An unreachable Hub resolves to `{ ok: false }` and the layout shows the error state.
 * @type {import('./$types').LayoutLoad}
 */
export async function load({ fetch, depends }) {
  depends(SNAPSHOT_DEPENDENCY);
  return { hub: await loadSnapshot(fetch) };
}
