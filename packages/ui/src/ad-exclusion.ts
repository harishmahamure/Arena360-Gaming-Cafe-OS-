// The shared "ad-exclusion" registry (Story 4.13). A surface staff are working in (a tile sheet,
// the settle bar, an Undo toast, an inline PIN, the More sheet, a dialog, later the POS cart)
// registers itself for as long as it is open. The Hub asks for the open list when the idle lock
// starts; while any is open the interstitial for that unlock is skipped. Nothing here knows about
// ads beyond the name: it is a counted set of surface names.

const open = new Map<string, number>();

/** Marks `surface` open until the returned function is called. Safe to register twice. */
export function registerAdExclusion(surface: string): () => void {
  open.set(surface, (open.get(surface) ?? 0) + 1);
  let done = false;
  return () => {
    if (done) return;
    done = true;
    const left = (open.get(surface) ?? 1) - 1;
    if (left <= 0) open.delete(surface);
    else open.set(surface, left);
  };
}

/** The names of the surfaces open right now (sorted), empty when none is. */
export function openAdExclusions(): string[] {
  return [...open.keys()].sort();
}
