import type {
  FoundPrinter,
  PrinterFindResult,
  PrinterErrorKind,
  PrinterHealth,
  PrinterReady,
  PrinterSelected,
  PrinterStatus,
  PrinterStatusReply,
  UiError,
} from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { authHeaders, errorOf, getToken, noticeLocked, setToken } from "./auth";
import { bridge } from "./service.svelte";

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
export const HEALTH_URL = "http://127.0.0.1:7409/ui/printer/health";
export const READY_URL = "http://127.0.0.1:7409/ui/printer/ready";
const READY_TIMEOUT_MS = 4_000;
const BLUETOOTH_ANSWER_MS = 60_000;
const FIND_TIMEOUT_MS = 15_000;
const PRINT_TIMEOUT_MS = 12_000;

export type PrinterCall<T> =
  | { kind: "ok"; body: T }
  | { kind: "signed_out" }
  | { kind: "error"; error: UiError }
  | { kind: "unreachable" };

async function call<T>(
  url: string,
  timeoutMs: number,
  method: "GET" | "POST" = "POST",
): Promise<PrinterCall<T>> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  let res: Response;
  try {
    res = await fetch(url, {
      method,
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
export const printerHealth = () => call<PrinterHealth>(HEALTH_URL, PRINT_TIMEOUT_MS, "GET");
/**
 * Whether the settle bar offers Print, and the branch's default receipt choice (Story 5.17). With
 * the profiled player the sale is for, it also says whether WhatsApp is offered (Story 5.19).
 */
export const printerReady = (playerId?: string | null) => {
  const url = new URL(READY_URL);
  if (playerId) url.searchParams.set("player_id", playerId);
  return call<PrinterReady>(url.toString(), READY_TIMEOUT_MS, "GET");
};

/**
 * On the Android Hub, asks for the Bluetooth permission (Android 12+) when it is not granted yet
 * and waits for the answer. Resolves at once on a Hub with no Bluetooth bridge or when it is
 * already granted; the Hub reports what it may do in the find result, so a refusal needs no
 * return value here.
 */
export async function askBluetooth(answerMs = BLUETOOTH_ANSWER_MS): Promise<void> {
  const hub = bridge();
  if (!hub?.bluetoothState || !hub.requestBluetooth || hub.bluetoothState() === "granted") return;
  await new Promise<void>((resolve) => {
    const done = () => {
      clearTimeout(timer);
      window.removeEventListener("arena:bluetooth", done);
      resolve();
    };
    const timer = setTimeout(done, answerMs);
    window.addEventListener("arena:bluetooth", done);
    hub.requestBluetooth?.();
  });
}

export function openAppSettings(): void {
  bridge()?.openAppSettings?.();
}

export function parseSelected(value: string | undefined): PrinterSelected | null {
  if (!value) return null;
  try {
    const p = JSON.parse(value) as Partial<PrinterSelected>;
    return (p.kind === "usb" || p.kind === "lan" || p.kind === "bluetooth") &&
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
  return {
    kind: p.kind,
    id_or_addr: p.kind === "usb" ? p.id : (p.addr ?? p.id),
    name: p.name,
  };
}

export function sameSelection(a: PrinterSelected | null, b: PrinterSelected | null): boolean {
  return !!a && !!b && a.kind === b.kind && a.id_or_addr === b.id_or_addr;
}

/** The message for each way a print can fail. */
export const PRINTER_ERROR_MESSAGES: Record<PrinterErrorKind, MessageKey> = {
  refused: "hub.settings.printer.error.refused",
  timeout: "hub.settings.printer.error.timeout",
  unreachable: "hub.settings.printer.error.unreachable",
  spooler: "hub.settings.printer.error.spooler",
  bluetooth: "hub.settings.printer.error.bluetooth",
  bluetooth_permission: "hub.settings.printer.error.bluetooth_permission",
  other: "hub.settings.printer.error.other",
};
