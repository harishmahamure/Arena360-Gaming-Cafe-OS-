/**
 * The Android Back stack. Every layer that Back closes (the More sheet, the alert list, a tile
 * sheet, the lock screen) registers a handler while it is open; the Kotlin shell sends an
 * `arena:back` window event for each Back gesture or button, and only the topmost handler runs.
 * With nothing registered Back does nothing: the Hub never exits or navigates on Back.
 */
export const BACK_EVENT = "arena:back";

type Handler = () => void;

const stack: Handler[] = [];

/** Registers `run` as the topmost Back handler; the returned function removes it. */
export function pushBack(run: Handler): () => void {
  const entry: Handler = () => run();
  stack.push(entry);
  return () => {
    const at = stack.indexOf(entry);
    if (at >= 0) stack.splice(at, 1);
  };
}

/** Runs the topmost handler; false when nothing is open. */
export function handleBack(): boolean {
  const top = stack[stack.length - 1];
  if (!top) return false;
  top();
  return true;
}

/** How many layers are registered (for tests). */
export const backDepth = () => stack.length;

/** Runs the top handler for each `arena:back` event on `target` (the window, in the app). */
export function listenForBack(target: Pick<EventTarget, "addEventListener">) {
  target.addEventListener(BACK_EVENT, () => void handleBack());
}

if (typeof window !== "undefined") listenForBack(window);
