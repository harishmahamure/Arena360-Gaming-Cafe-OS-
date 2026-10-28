import type {
  ImportTemplateSaved,
  ProductImportPreview,
  ProductImportResult,
} from "@arena/ui/generated";
import { authHeaders, getToken, setToken } from "./auth";

export const IMPORT_URL = "http://127.0.0.1:7409/ui/import/products";

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

/** `POST /ui/import/products/preview`: a dry-run of `csv`. Nothing is written. */
export const previewProducts = (csv: string) =>
  post<ProductImportPreview>("preview", { csv }, (b) => hasNumber(b, "rows"), 30_000);

/**
 * `POST /ui/import/products/commit`: applies the file in chunks of 500 rows. `importId` names
 * this attempt; asking again with the same id never adds anything twice.
 */
export const commitProducts = (csv: string, importId: string, validOnly: boolean) =>
  post<ProductImportResult>(
    "commit",
    { csv, import_id: importId, valid_only: validOnly },
    (b) => hasNumber(b, "chunks"),
    120_000,
  );

/** `POST /ui/import/products/template`: saves the template CSV into `folder` on the Hub. */
export const saveProductTemplate = (folder: string) =>
  post<ImportTemplateSaved>(
    "template",
    { folder },
    (b) =>
      typeof b === "object" && b !== null && typeof (b as ImportTemplateSaved).path === "string",
    10_000,
  );
