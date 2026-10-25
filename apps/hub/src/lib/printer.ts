import type {
  FoundPrinter,
  PrinterFindResult,
  PrinterSelected,
  PrinterStatus,
  PrinterStatusReply,
  UiError,
} from "@arena/ui/generated";
import { authHeaders, errorOf, getToken, noticeLocked, setToken } from "./auth";

export const KEY_PRINTER_SELECTED = "printer.selected";
export const KEY_PRINTER_PAPER_MM = "printer.paper_mm";
export const KEY_PRINTER_STATUS = "printer.status";
export const PRINTER_KEYS = [KEY_PRINTER_SELECTED, KEY_PRINTER_PAPER_MM, KEY_PRINTER_STATUS];
export const KEY_LICENCE_FLAGS = "licence.flags";
export const KEY_KOT_MODE = "kitchen.kot_mode";
export const KEY_KOT_PRINTER = "kitchen.kot_printer";
export const KEY_KOT_PAPER_MM = "kitchen.kot_paper_mm";
export const KOT_MODES = ["receipt", "another", "none"] as const;
export const KITCHEN_KEYS = [KEY_LICENCE_FLAGS, KEY_KOT_MODE, KEY_KOT_PRINTER, KEY_KOT_PAPER_MM];
export const PAPER_WIDTHS = ["58", "80"] as const;
export const FIND_URL = "http://127.0.0.1:7409/ui/printer/find";
export const TEST_URL = "http://127.0.0.1:7409/ui/printer/test";
export const KOT_TEST_URL = "http://127.0.0.1:7409/ui/printer/test-kot";
export const SKIP_URL = "http://127.0.0.1:7409/ui/printer/skip";
const FIND_TIMEOUT_MS = 15_000;
const PRINT_TIMEOUT_MS = 12_000;

export type PrinterCall<T> =
  | { kind: "ok"; body: T }
  | { kind: "signed_out" }
  | { kind: "error"; error: UiError }
  | { kind: "unreachable" };

async function call<T>(url: string, timeoutMs: number): Promise<PrinterCall<T>> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  let res: Response;
  try {
    res = await fetch(url, {
      method: "POST",
      headers: authHeaders(token),
      signal: AbortSignal.timeout(timeoutMs),
    });
  } catch {
    return { kind: "unreachable" };
  }
  if (res.status === 401) {
    setToken(null);
    return { kind: "signed_out" };
  }
  if (res.ok) {
    try {
      return { kind: "ok", body: (await res.json()) as T };
    } catch {
      return { kind: "unreachable" };
    }
  }
  const error = await errorOf(res);
  noticeLocked(error);
  return error ? { kind: "error", error } : { kind: "unreachable" };
}

export const findPrinters = () => call<PrinterFindResult>(FIND_URL, FIND_TIMEOUT_MS);
export const testPrint = () => call<PrinterStatusReply>(TEST_URL, PRINT_TIMEOUT_MS);
export const testKotPrint = () => call<PrinterStatusReply>(KOT_TEST_URL, PRINT_TIMEOUT_MS);
export const skipPrinter = () => call<PrinterStatusReply>(SKIP_URL, PRINT_TIMEOUT_MS);

export function parseSelected(value: string | undefined): PrinterSelected | null {
  if (!value) return null;
  try {
    const p = JSON.parse(value) as Partial<PrinterSelected>;
    return (p.kind === "usb" || p.kind === "lan") &&
      typeof p.id_or_addr === "string" &&
      typeof p.name === "string"
      ? (p as PrinterSelected)
      : null;
  } catch {
    return null;
  }
}

export function parseStatus(value: string | undefined): PrinterStatus | null {
  if (!value) return null;
  try {
    const s = JSON.parse(value) as Partial<PrinterStatus>;
    return typeof s.state === "string" ? (s as PrinterStatus) : null;
  } catch {
    return null;
  }
}

export function selectionOf(p: FoundPrinter): PrinterSelected {
  return { kind: p.kind, id_or_addr: p.kind === "lan" ? (p.addr ?? p.id) : p.id, name: p.name };
}

export function sameSelection(a: PrinterSelected | null, b: PrinterSelected | null): boolean {
  return !!a && !!b && a.kind === b.kind && a.id_or_addr === b.id_or_addr;
}
