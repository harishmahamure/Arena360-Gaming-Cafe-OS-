import type {
  ImportTemplateSaved,
  PlanImportPreview,
  PlanImportResult,
  PlayerImportPreview,
  PlayerImportResult,
  ProductImportPreview,
  ProductImportResult,
} from "@arena/ui/generated";
import { authHeaders, getToken, setToken } from "./auth";

export const IMPORT_URL = "http://127.0.0.1:7409/ui/import";

/** The largest file the Hub reads (it refuses more). */
export const MAX_IMPORT_BYTES = 1_500_000;

export type ImportOutcome<T> =
  | { kind: "ok"; view: T }
  | { kind: "invalid"; field: string; message: string }
  | { kind: "denied" }
  | { kind: "signed_out" }
  | { kind: "unreachable" };

/** Posts `body` to the import API: the view on 200, the refused field and message on 422. */
async function post<T>(
  path: string,
  body: unknown,
  isView: (b: unknown) => boolean,
  timeoutMs: number,
): Promise<ImportOutcome<T>> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const url = `${IMPORT_URL}/${path}`; // i18n-ignore (a URL)
    const res = await fetch(url, {
      method: "POST",
      headers: { ...authHeaders(token), "content-type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(timeoutMs),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    if (res.status === 403) return { kind: "denied" };
    const answer: unknown = await res.json().catch(() => null);
    if (res.status === 422 && typeof answer === "object" && answer !== null) {
      const e = answer as { field?: unknown; message?: unknown };
      return {
        kind: "invalid",
        field: typeof e.field === "string" ? e.field : "",
        message: typeof e.message === "string" ? e.message : "",
      };
    }
    return res.ok && isView(answer) ? { kind: "ok", view: answer as T } : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

const hasNumber = (b: unknown, key: string) =>
  typeof b === "object" && b !== null && typeof (b as Record<string, unknown>)[key] === "number";

export type ImportKind = "products" | "players" | "plans";

/** `POST /ui/import/<kind>/preview`: a dry-run of `csv`. Nothing is written. */
const preview =
  <T>(kind: ImportKind) =>
  (csv: string) =>
    post<T>(`${kind}/preview`, { csv }, (b) => hasNumber(b, "rows"), 30_000); // i18n-ignore (a URL)

/**
 * `POST /ui/import/<kind>/commit`: applies the file in chunks of 500 rows. `importId` names
 * this attempt; asking again with the same id never adds anything twice.
 */
const commit =
  <T>(kind: ImportKind) =>
  (csv: string, importId: string, validOnly: boolean) =>
    post<T>(
      `${kind}/commit`, // i18n-ignore (a URL)
      { csv, import_id: importId, valid_only: validOnly },
      (b) => hasNumber(b, "chunks"),
      120_000,
    );

/** `POST /ui/import/<kind>/template`: saves the template CSV into `folder` on the Hub. */
const template = (kind: ImportKind) => (folder: string) =>
  post<ImportTemplateSaved>(
    `${kind}/template`, // i18n-ignore (a URL)
    { folder },
    (b) =>
      typeof b === "object" && b !== null && typeof (b as ImportTemplateSaved).path === "string",
    10_000,
  );

export const previewProducts = preview<ProductImportPreview>("products");
export const commitProducts = commit<ProductImportResult>("products");
export const saveProductTemplate = template("products");
export const previewPlayers = preview<PlayerImportPreview>("players");
export const commitPlayers = commit<PlayerImportResult>("players");
export const savePlayerTemplate = template("players");
export const previewPlans = preview<PlanImportPreview>("plans");
export const commitPlans = commit<PlanImportResult>("plans");
export const savePlanTemplate = template("plans");
