import type { RestoreFetchView, RestoreRebuildView } from "@arena/ui/generated";
import { errorOf } from "./auth";

/** The loopback routes behind "Restore from cloud backup" (Story 9.12): no session needed. */
export const RESTORE_CLOUD_URL = "http://127.0.0.1:7409/ui/restore/cloud";
const TIMEOUT_MS = 15000;

export type RestoreCall<V> =
  | { kind: "ok"; view: V | null }
  /** The Hub refused: `KEY_NOT_RELEASED`, `FEATURE_NOT_ENTITLED`, `NOT_FOUND`, … */
  | { kind: "refused"; code: string }
  | { kind: "hub_unreachable" };

export type RestoreCloudResult = RestoreCall<RestoreFetchView>;
export type RestoreRebuildResult = RestoreCall<RestoreRebuildView>;

async function call<V>(path: string, method: "GET" | "POST"): Promise<RestoreCall<V>> {
  let res: Response;
  try {
    res = await fetch(RESTORE_CLOUD_URL + path, {
      method,
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
  } catch {
    return { kind: "hub_unreachable" };
  }
  if (res.ok) {
    try {
      return { kind: "ok", view: (await res.json()) as V | null };
    } catch {
      return { kind: "hub_unreachable" };
    }
  }
  const err = await errorOf(res);
  return err ? { kind: "refused", code: err.code } : { kind: "hub_unreachable" };
}

/** Where the download stands, or `null` before one starts. */
export const restoreCloudView = () => call<RestoreFetchView>("", "GET");
/** Starts the download, or resumes it from the files already staged. */
export const startRestoreCloud = () => call<RestoreFetchView>("", "POST");
/** Stops it and removes the staging folder. */
export const cancelRestoreCloud = () => call<RestoreFetchView>("/cancel", "POST");
/** After a gap: keeps what is before it. */
export const restoreCloudPartial = () => call<RestoreFetchView>("/partial", "POST");

/** Where the rebuild from the downloaded backup stands (Story 9.13), or `null` before one starts. */
export const restoreRebuildView = () => call<RestoreRebuildView>("/rebuild", "GET");
/** Rebuilds the branch from the downloaded backup, then the Hub restarts onto it. */
export const startRestoreRebuild = () => call<RestoreRebuildView>("/rebuild", "POST");
/** Stops the rebuild before the restart; the downloaded backup is kept. */
export const cancelRestoreRebuild = () => call<RestoreRebuildView>("/rebuild/cancel", "POST");
