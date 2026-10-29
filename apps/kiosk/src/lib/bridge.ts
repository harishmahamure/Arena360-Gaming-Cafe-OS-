// The webview's only link to the outside: the Tauri shell, which relays the Kiosk service.
// The webview has no Hub socket and no token; it hears the Hub link state as the `hub-status`
// event and reports its own restorable state, which comes back as `restore-state` after a
// restart. Toasts the service raises (a blocked app was closed) arrive as `toast`, and the
// high-contrast setting a manager chose for this kiosk as `theme`, and the idle screen's view and
// logo as `attract`. Outside Tauri (a plain browser in dev) every call is a no-op.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AttractView,
  DiscoveredHub,
  KioskHubState,
  KioskHubStatus,
  PlayerChoice,
} from "@arena/ui/generated";
// The event and command names, shared with the Rust shell (its tests read the same file).
import names from "../../src-tauri/ipc-names.json" with { type: "json" };
import { LOGIN_TIMEOUT_MS, outcomeOf, type LoginOutcome } from "./login";
import { PICKER_TIMEOUT_MS, plansOf, startOf, type PlansOutcome, type StartOutcome } from "./plans";
import { SIGNUP_TIMEOUT_MS, signupOutcomeOf, type SignupOutcome } from "./signup";

export const HUB_STATUS_EVENT = names.events.hubStatus;
export const RESTORE_STATE_EVENT = names.events.restoreState;
export const TOAST_EVENT = names.events.toast;
export const ADMIN_EXIT_EVENT = names.events.adminExit;
export const ADMIN_EXIT_ENDED_EVENT = names.events.adminExitEnded;
export const THEME_EVENT = names.events.theme;
export const PC_SCREEN_EVENT = names.events.pcScreen;
export const ATTRACT_EVENT = names.events.attract;
export const TEST_SESSION_EVENT = names.events.testSession;
export const TEST_SESSION_ENDED_EVENT = names.events.testSessionEnded;
export const DISCOVERY_FOUND_EVENT = names.events.discoveryFound;
export const DISCOVERY_DONE_EVENT = names.events.discoveryDone;
export const HUD_EXPAND_EVENT = names.events.hudExpand;
const CMD = names.commands;
/** How often the webview tells the shell it is alive (the shell answers pings only then). */
export const HEARTBEAT_MS = 1000;

const STATES: readonly KioskHubState[] = [
  "connected",
  "reconnecting",
  "token_rejected",
  "unpaired",
];

const inTauri = (): boolean => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export function isHubStatus(v: unknown): v is KioskHubStatus {
  return (
    typeof v === "object" &&
    v !== null &&
    STATES.includes((v as { state?: unknown }).state as KioskHubState)
  );
}

/** The link state the shell already knows, if any. */
export async function currentHubStatus(): Promise<KioskHubStatus | null> {
  if (!inTauri()) return null;
  try {
    const v: unknown = await invoke(CMD.hubStatus);
    return isHubStatus(v) ? v : null;
  } catch {
    return null;
  }
}

/** Calls `cb` on every link change; resolves to the unsubscribe function. */
export async function onHubStatus(cb: (s: KioskHubStatus) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(HUB_STATUS_EVENT, (e) => {
    if (isHubStatus(e.payload)) cb(e.payload);
  });
}

/** Calls `cb` with the state to restore whenever the service sends one. */
export async function onRestoreState(cb: (state: unknown) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(RESTORE_STATE_EVENT, (e) => cb(e.payload));
}

/** The toasts the service may raise (catalogue keys); anything else is ignored. */
export const TOAST_KEYS = ["player.app.blocked"] as const;
export type ToastKey = (typeof TOAST_KEYS)[number];

/** How each toast behaves on the HUD (Story 6.14): an info toast goes after 8 s, a warning stays. */
export const TOAST_KINDS: Record<ToastKey, "info" | "warning"> = {
  "player.app.blocked": "warning",
};

export function isToastKey(v: unknown): v is ToastKey {
  return typeof v === "string" && (TOAST_KEYS as readonly string[]).includes(v);
}

/** Calls `cb` with every toast the service raises (e.g. after it closed a blocked app). */
export async function onToast(cb: (key: ToastKey) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(TOAST_EVENT, (e) => {
    if (isToastKey(e.payload)) cb(e.payload);
  });
}

/** A state to restore that arrived before this page listened, once. */
export async function takeRestoreState(): Promise<unknown> {
  if (!inTauri()) return null;
  try {
    return await invoke(CMD.takeRestoreState);
  } catch {
    return null;
  }
}

/** Sends a heartbeat every second; resolves to the stop function. */
export function startHeartbeat(): () => void {
  if (!inTauri()) return () => {};
  const beat = () => void invoke(CMD.heartbeat).catch(() => {});
  beat();
  const timer = setInterval(beat, HEARTBEAT_MS);
  return () => clearInterval(timer);
}

/** The app's restorable state: where the player is (path and query). */
export type KioskAppState = { route: string };

/** Sends the app's state to the service (kept there across an app restart). */
export function reportAppState(state: KioskAppState): void {
  if (!inTauri()) return;
  void invoke(CMD.reportAppState, { state }).catch(() => {});
}

/** The in-app route a restored state names, or `null` when it names none (or anything else). */
export function restoredRoute(v: unknown): string | null {
  if (typeof v !== "object" || v === null) return null;
  const route = (v as { route?: unknown }).route;
  if (typeof route !== "string" || !route.startsWith("/")) return null;
  // Never another origin: no `//host` or `/\host`.
  if (route.startsWith("//") || route.includes("\\")) return null;
  return route;
}

/** What the service answered to an admin-exit PIN (Story 1.20). */
export type AdminExitResult =
  | { kind: "granted"; until: number }
  | { kind: "rejected" }
  | { kind: "locked"; retryAfterS: number }
  | { kind: "unreachable" };

type Answer = { ok: boolean; code?: string; body: unknown };

const isAnswer = (v: unknown): v is Answer =>
  typeof v === "object" && v !== null && typeof (v as { ok?: unknown }).ok === "boolean";

const field = (body: unknown, name: string): number | null => {
  const v =
    typeof body === "object" && body !== null ? (body as Record<string, unknown>)[name] : null;
  return typeof v === "number" && Number.isFinite(v) ? v : null;
};

/**
 * Sends a manager PIN for an admin exit to the Kiosk service (through the shell, never the
 * Hub directly). The caller drops its own copy of the PIN.
 */
export async function adminExit(pin: string): Promise<AdminExitResult> {
  if (!inTauri()) return { kind: "unreachable" };
  try {
    const answer: unknown = await invoke(CMD.adminExit, { pin });
    if (!isAnswer(answer)) return { kind: "unreachable" };
    if (answer.ok) {
      const until = field(answer.body, "until");
      return until === null ? { kind: "unreachable" } : { kind: "granted", until };
    }
    if (answer.code === "AUTH_LOCKED") {
      return {
        kind: "locked",
        retryAfterS: Math.max(1, field(answer.body, "retry_after_s") ?? 60),
      };
    }
    if (answer.code === "PIN_REJECTED") return { kind: "rejected" };
    return { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

/** Whether the admin-exit prompt is paused (seconds left) and whether an exit runs (until). */
export async function adminExitStatus(): Promise<{
  retryAfterS: number | null;
  until: number | null;
}> {
  const none = { retryAfterS: null, until: null };
  if (!inTauri()) return none;
  try {
    const answer: unknown = await invoke(CMD.adminExitStatus);
    if (!isAnswer(answer) || !answer.ok) return none;
    return { retryAfterS: field(answer.body, "retry_after_s"), until: field(answer.body, "until") };
  } catch {
    return none;
  }
}

/** Calls `cb` with `until` (UTC ms) whenever an admin exit starts. */
export async function onAdminExit(cb: (until: number) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(ADMIN_EXIT_EVENT, (e) => {
    const until = field(e.payload, "until");
    if (until !== null) cb(until);
  });
}

/** Calls `cb` whenever the admin exit ends. */
export async function onAdminExitEnded(cb: () => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(ADMIN_EXIT_ENDED_EVENT, () => cb());
}

export async function currentTestSession(): Promise<number | null> {
  if (!inTauri()) return null;
  try {
    const v: unknown = await invoke(CMD.testSession);
    return typeof v === "number" && Number.isFinite(v) ? v : null;
  } catch {
    return null;
  }
}

export async function onTestSession(cb: (remainingMs: number) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(TEST_SESSION_EVENT, (e) => {
    const ms = field(e.payload, "remaining_ms");
    if (ms !== null) cb(ms);
  });
}

export async function onTestSessionEnded(cb: () => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(TEST_SESSION_ENDED_EVENT, () => cb());
}

export type PcScreen = {
  locked: boolean;
  reason: string | null;
  message: string | null;
  session: {
    device_name: string;
    elapsed_ms: number;
    warning_min: number | null;
    grace_in_ms: number | null;
    grace_ms: number;
    ordered_items: number;
    username: string | null;
    guest_no: number | null;
    remaining_ms: number | null;
    /** The wallet balance in minor units (a profiled player), for the HUD. */
    balance_minor: number | null;
    /** The pack or pass the session runs on, for the HUD. */
    plan_name: string | null;
  } | null;
};

const screenOf = (v: unknown): PcScreen | null => {
  if (typeof v !== "object" || v === null) return null;
  const { locked, reason, message, session } = v as Record<string, unknown>;
  if (typeof locked !== "boolean") return null;
  if (message != null && typeof message !== "string") return null;
  const s = session as
    | {
        device_name?: unknown;
        elapsed_ms?: unknown;
        warning_min?: unknown;
        grace_in_ms?: unknown;
        grace_ms?: unknown;
        ordered_items?: unknown;
        username?: unknown;
        guest_no?: unknown;
        remaining_ms?: unknown;
        balance_minor?: unknown;
        plan_name?: unknown;
      }
    | null
    | undefined;
  const live =
    s && typeof s.device_name === "string" && typeof s.elapsed_ms === "number"
      ? {
          device_name: s.device_name,
          elapsed_ms: s.elapsed_ms,
          warning_min: typeof s.warning_min === "number" ? s.warning_min : null,
          grace_in_ms: typeof s.grace_in_ms === "number" ? s.grace_in_ms : null,
          grace_ms: typeof s.grace_ms === "number" ? s.grace_ms : 0,
          ordered_items: typeof s.ordered_items === "number" ? s.ordered_items : 0,
          username: typeof s.username === "string" ? s.username : null,
          guest_no: typeof s.guest_no === "number" ? s.guest_no : null,
          remaining_ms: typeof s.remaining_ms === "number" ? s.remaining_ms : null,
          balance_minor: typeof s.balance_minor === "number" ? s.balance_minor : null,
          plan_name: typeof s.plan_name === "string" ? s.plan_name : null,
        }
      : null;
  return {
    locked,
    reason: typeof reason === "string" ? reason : null,
    message: message ?? null,
    session: live,
  };
};

export async function currentPcScreen(): Promise<PcScreen | null> {
  if (!inTauri()) return null;
  try {
    return screenOf(await invoke(CMD.pcScreen));
  } catch {
    return null;
  }
}

export async function onPcScreen(cb: (s: PcScreen) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(PC_SCREEN_EVENT, (e) => {
    const s = screenOf(e.payload);
    if (s) cb(s);
  });
}

/** What the idle screen shows: the last attract view and the branch logo (a `data:` URI). */
export type AttractShown = { view: AttractView | null; logo: string | null };

const ATTRACT_STATES = ["ready", "ask_counter"];

const isLocale = (v: unknown): boolean => {
  if (typeof v !== "object" || v === null) return false;
  const l = v as Record<string, unknown>;
  return (
    typeof l.currency === "string" &&
    typeof l.exponent === "number" &&
    typeof l.symbol === "string" &&
    typeof l.tz === "string"
  );
};

const isRule = (v: unknown): boolean => {
  if (typeof v !== "object" || v === null) return false;
  const r = v as Record<string, unknown>;
  return (
    typeof r.name === "string" &&
    typeof r.adjust_kind === "string" &&
    typeof r.adjust_value === "number" &&
    (r.until == null || typeof r.until === "number")
  );
};

const isAttractView = (v: unknown): v is AttractView => {
  if (typeof v !== "object" || v === null) return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.branch_name === "string" &&
    typeof o.state === "string" &&
    ATTRACT_STATES.includes(o.state) &&
    isLocale(o.locale) &&
    (o.rate_minor == null || typeof o.rate_minor === "number") &&
    (o.rule == null || isRule(o.rule)) &&
    Array.isArray(o.media) &&
    o.media.every(
      (m) =>
        typeof m === "object" && m !== null && typeof (m as { video?: unknown }).video === "string",
    )
  );
};

export const attractOf = (v: unknown): AttractShown | null => {
  if (typeof v !== "object" || v === null) return null;
  const { view, logo } = v as Record<string, unknown>;
  if (view != null && !isAttractView(view)) return null;
  if (logo != null && typeof logo !== "string") return null;
  return {
    view: (view as AttractView | null | undefined) ?? null,
    logo: (logo as string | null | undefined) ?? null,
  };
};

/** The attract view and logo the shell already knows (it keeps the latest, from the cache). */
export async function currentAttract(): Promise<AttractShown | null> {
  if (!inTauri()) return null;
  try {
    return attractOf(await invoke(CMD.attract));
  } catch {
    return null;
  }
}

/** Calls `cb` with the attract view and logo whenever either changes. */
export async function onAttract(cb: (s: AttractShown) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(ATTRACT_EVENT, (e) => {
    const s = attractOf(e.payload);
    if (s) cb(s);
  });
}

/**
 * A player's username and password for the Hub to check (Story 6.8). They go to the Kiosk service
 * through the shell and nowhere else; the caller drops its own copy. Outside Tauri, on an error or
 * when nothing answers in time the outcome is `unreachable`.
 */
export async function playerLogin(username: string, password: string): Promise<LoginOutcome> {
  if (!inTauri()) return { kind: "unreachable" };
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const answer = await Promise.race([
      invoke<unknown>(CMD.playerLogin, { username, password }),
      new Promise<null>((resolve) => {
        timer = setTimeout(() => resolve(null), LOGIN_TIMEOUT_MS);
      }),
    ]);
    return outcomeOf(answer);
  } catch {
    return { kind: "unreachable" };
  } finally {
    clearTimeout(timer);
  }
}

/** What a new player typed at the sign-up form (Story 6.10). */
export type SignupFields = {
  username: string;
  password: string;
  phone: string;
  name: string;
  consent: boolean;
};

/**
 * A new player's sign-up for the Hub to create (Story 6.10). The fields go to the Kiosk service
 * through the shell and nowhere else; the caller drops its own copy. Outside Tauri, on an error or
 * when nothing answers in time the outcome is `unreachable`.
 */
export async function playerSignup(fields: SignupFields): Promise<SignupOutcome> {
  if (!inTauri()) return { kind: "unreachable" };
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const answer = await Promise.race([
      invoke<unknown>(CMD.playerSignup, { ...fields }),
      new Promise<null>((resolve) => {
        timer = setTimeout(() => resolve(null), SIGNUP_TIMEOUT_MS);
      }),
    ]);
    return signupOutcomeOf(answer);
  } catch {
    return { kind: "unreachable" };
  } finally {
    clearTimeout(timer);
  }
}

/** Calls the shell command `cmd`, giving up after PICKER_TIMEOUT_MS (`null`: no answer). */
async function ask(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      invoke<unknown>(cmd, args),
      new Promise<null>((resolve) => {
        timer = setTimeout(() => resolve(null), PICKER_TIMEOUT_MS);
      }),
    ]);
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

/**
 * The options valid for this PC for the player who just logged in (Story 6.9). The Hub knows who
 * that is; the kiosk names nobody. Outside Tauri, on an error or with no answer in time it is
 * `unreachable`; `login` means the Hub no longer holds the login.
 */
export async function playerPlans(): Promise<PlansOutcome> {
  if (!inTauri()) return { kind: "unreachable" };
  return plansOf(await ask(CMD.playerPlans));
}

/** Starts the chosen option for the logged-in player (Story 6.9); the Hub answers ok or a refusal. */
export async function playerStart(choice: PlayerChoice): Promise<StartOutcome> {
  if (!inTauri()) return { kind: "unreachable" };
  return startOf(await ask(CMD.playerStart, { choice }));
}

/** Tells the Hub to forget the logged-in player (best effort: the Hub also forgets after 120 s). */
export function playerLogout(): void {
  if (!inTauri()) return;
  void invoke(CMD.playerLogout).catch(() => {});
}

export function dismissPcMessage(): void {
  if (!inTauri()) return;
  void invoke(CMD.pcDismiss).catch(() => {});
}

/** Whether `e` is the hidden admin-exit combination, Ctrl+Alt+Shift+A. */
export function isAdminExitCombo(e: KeyboardEvent): boolean {
  return e.ctrlKey && e.altKey && e.shiftKey && !e.metaKey && e.code === "KeyA";
}

/**
 * The theme on `<html>`: high contrast or the default dark. Set by a manager per device on the
 * Hub (Story 1.21); the shell already set it before the first paint, so this only follows
 * changes. There is no player control and no light theme.
 */
export function applyTheme(hc: boolean): void {
  document.documentElement.dataset.theme = hc ? "hc" : "dark";
}

const hcOf = (v: unknown): boolean | null => {
  const hc = typeof v === "object" && v !== null ? (v as { hc?: unknown }).hc : v;
  return typeof hc === "boolean" ? hc : null;
};

/** The theme the shell knows now (`null` outside Tauri or when it does not answer). */
export async function currentTheme(): Promise<boolean | null> {
  if (!inTauri()) return null;
  try {
    return hcOf(await invoke(CMD.theme));
  } catch {
    return null;
  }
}

/** Calls `cb` with `hc` whenever this kiosk's theme changes. */
export async function onTheme(cb: (hc: boolean) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(THEME_EVENT, (e) => {
    const hc = hcOf(e.payload);
    if (hc !== null) cb(hc);
  });
}

export function isDiscoveredHub(v: unknown): v is DiscoveredHub {
  if (typeof v !== "object" || v === null) return false;
  const h = v as Record<string, unknown>;
  return (
    typeof h.addr === "string" &&
    typeof h.branch_name === "string" &&
    typeof h.hub_name === "string" &&
    typeof h.fingerprint === "string" &&
    typeof h.protocol_version === "number" &&
    typeof h.compatible === "boolean" &&
    typeof h.web_port === "number"
  );
}

export const DISCOVERY_FALLBACK_MS = 12_000;

export type DiscoveryEvent = { search: number; hubs: DiscoveredHub[] };

const searchOf = (payload: unknown): number | null => {
  const search =
    typeof payload === "object" && payload !== null
      ? (payload as { search?: unknown }).search
      : undefined;
  return typeof search === "number" && Number.isInteger(search) ? search : null;
};

const hubsOf = (payload: unknown): DiscoveredHub[] | null => {
  const hubs =
    typeof payload === "object" && payload !== null
      ? (payload as { hubs?: unknown }).hubs
      : undefined;
  return Array.isArray(hubs) ? hubs.filter(isDiscoveredHub) : null;
};

export async function onDiscoveryFound(cb: (e: DiscoveryEvent) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(DISCOVERY_FOUND_EVENT, (e) => {
    const hubs = hubsOf(e.payload);
    const search = searchOf(e.payload);
    if (hubs && search !== null) cb({ search, hubs });
  });
}

export async function onDiscoveryDone(cb: (e: DiscoveryEvent) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(DISCOVERY_DONE_EVENT, (e) => {
    const search = searchOf(e.payload);
    if (search !== null) cb({ search, hubs: hubsOf(e.payload) ?? [] });
  });
}

export async function startDiscovery(): Promise<number | null> {
  if (!inTauri()) return null;
  try {
    const answer: unknown = await invoke(CMD.discover);
    if (!isAnswer(answer) || !answer.ok) return null;
    return searchOf(answer.body);
  } catch {
    return null;
  }
}

export type ProbeResult =
  | { kind: "found"; hub: DiscoveredHub }
  | { kind: "no_hub" }
  | { kind: "incompatible" }
  | { kind: "unreachable" };

export async function probeIp(ip: string): Promise<ProbeResult> {
  if (!inTauri()) return { kind: "unreachable" };
  try {
    const answer: unknown = await invoke(CMD.probeIp, { ip });
    if (!isAnswer(answer)) return { kind: "unreachable" };
    if (answer.ok) {
      const hub =
        typeof answer.body === "object" && answer.body !== null
          ? (answer.body as { hub?: unknown }).hub
          : undefined;
      return isDiscoveredHub(hub) ? { kind: "found", hub } : { kind: "unreachable" };
    }
    if (answer.code === "HUB_INCOMPATIBLE") return { kind: "incompatible" };
    if (answer.code === "NO_HUB") return { kind: "no_hub" };
    return { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

export type SelectResult = "selected" | "no_hub" | "unreachable";

export async function selectHub(hub: DiscoveredHub): Promise<SelectResult> {
  if (!inTauri()) return "unreachable";
  try {
    const answer: unknown = await invoke(CMD.selectHub, {
      addr: hub.addr,
      fingerprint: hub.fingerprint,
    });
    if (!isAnswer(answer)) return "unreachable";
    if (answer.ok) return "selected";
    return answer.code === "NO_HUB" ? "no_hub" : "unreachable";
  } catch {
    return "unreachable";
  }
}

export type PairInput = { code: string } | { pin: string };

export type PairResult =
  | { kind: "paired"; branchName: string; deviceId: string }
  | { kind: "code_invalid" }
  | { kind: "pin_rejected" }
  | { kind: "device_limit" }
  | { kind: "rate_limited" }
  | { kind: "pin_paused"; retryAfterS: number }
  | { kind: "no_hub" }
  | { kind: "invalid" }
  | { kind: "not_primary" }
  | { kind: "hub_slow" }
  | { kind: "hub_error" }
  | { kind: "already_paired" }
  | { kind: "unreachable" };

const text = (body: unknown, name: string): string | null => {
  const v =
    typeof body === "object" && body !== null ? (body as Record<string, unknown>)[name] : null;
  return typeof v === "string" ? v : null;
};

export async function pairKiosk(input: PairInput, pcName: string): Promise<PairResult> {
  if (!inTauri()) return { kind: "unreachable" };
  try {
    const answer: unknown = await invoke(CMD.pair, { ...input, pcName });
    if (!isAnswer(answer)) return { kind: "unreachable" };
    if (answer.ok) {
      const branchName = text(answer.body, "branch_name");
      const deviceId = text(answer.body, "device_id");
      return branchName !== null && deviceId !== null
        ? { kind: "paired", branchName, deviceId }
        : { kind: "unreachable" };
    }
    switch (answer.code) {
      case "PAIR_CODE_INVALID":
        return { kind: "code_invalid" };
      case "PIN_REJECTED":
        return { kind: "pin_rejected" };
      case "PAIR_PIN_PAUSED":
        return {
          kind: "pin_paused",
          retryAfterS: Math.max(1, field(answer.body, "retry_after_s") ?? 60),
        };
      case "DEVICE_LIMIT":
        return { kind: "device_limit" };
      case "RATE_LIMITED":
        return { kind: "rate_limited" };
      case "NO_HUB":
        return { kind: "no_hub" };
      case "VALIDATION_FAILED":
      case "BAD_FRAME":
        return { kind: "invalid" };
      case "NOT_PRIMARY":
        return { kind: "not_primary" };
      case "HUB_SLOW":
        return { kind: "hub_slow" };
      case "INTERNAL":
        return { kind: "hub_error" };
      case "ALREADY_SET":
        return { kind: "already_paired" };
      default:
        return { kind: "unreachable" };
    }
  } catch {
    return { kind: "unreachable" };
  }
}

export async function pairInfo(): Promise<{ pcName: string | null }> {
  if (!inTauri()) return { pcName: null };
  try {
    const answer: unknown = await invoke(CMD.pairInfo);
    if (!isAnswer(answer) || !answer.ok) return { pcName: null };
    return { pcName: text(answer.body, "pc_name") };
  } catch {
    return { pcName: null };
  }
}

/** Calls `cb` when Ctrl+Shift+E asks the HUD to expand into its panel (Story 6.14). */
export async function onHudExpand(cb: () => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  return listen<unknown>(HUD_EXPAND_EVENT, () => cb());
}

/**
 * Tells the shell how big the HUD's content is, in logical px (`0 × 0`: nothing to draw), so it
 * can place, show or hide the always-on-top HUD window.
 */
export function hudLayout(width: number, height: number): void {
  if (!inTauri()) return;
  void invoke(CMD.hudLayout, { width, height }).catch(() => {});
}

/** Esc in the expanded panel: the shell makes the window non-focusable and returns the focus. */
export function hudCollapse(): void {
  if (!inTauri()) return;
  void invoke(CMD.hudCollapse).catch(() => {});
}
