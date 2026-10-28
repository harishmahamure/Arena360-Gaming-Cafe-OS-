// The branch game and app library for Settings → Games (Story 6.3): the list the Hub serves at
// `GET /ui/library` and the small parsing the form needs. Entries are written by `library.upsert`
// and `library.delete`; the Hub shows a catalogue-linked entry with the catalogue's name,
// launcher and age rating.

import type {
  DeviceScanView,
  LibraryEntryView,
  LibraryScansView,
  LibraryView,
} from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import { authHeaders, getToken, setToken } from "./auth";

export const LIBRARY_URL = "http://127.0.0.1:7409/ui/library";
/** Each PC's last installed-software scan (Story 6.4). */
export const SCANS_URL = "http://127.0.0.1:7409/ui/library/scans";

export const KINDS = ["game", "app"] as const;
export const LAUNCHERS = ["steam", "epic", "riot", "battlenet", "none"] as const;
export const NAME_MAX = 80;
export const PATH_MAX = 520;
export const URI_MAX = 512;
export const ARGS_MAX = 512;
export const CATEGORY_MAX = 40;
export const TAGS_MAX = 10;
export const TAG_MAX = 32;
export const AGE_MAX = 16;
export const CATALOG_ID_MAX = 64;

export const KIND_LABELS: Record<(typeof KINDS)[number], MessageKey> = {
  game: "hub.games.kind.game",
  app: "hub.games.kind.app",
};

export const LAUNCHER_LABELS: Record<(typeof LAUNCHERS)[number], MessageKey> = {
  steam: "hub.games.launcher.steam",
  epic: "hub.games.launcher.epic",
  riot: "hub.games.launcher.riot",
  battlenet: "hub.games.launcher.battlenet",
  none: "hub.games.launcher.none",
};

export type LibraryOutcome =
  { kind: "ok"; entries: LibraryEntryView[] } | { kind: "signed_out" } | { kind: "unreachable" };

export async function loadLibrary(): Promise<LibraryOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(LIBRARY_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    const body: unknown = res.ok ? await res.json() : null;
    const view = body as LibraryView | null;
    return view && Array.isArray(view.entries)
      ? { kind: "ok", entries: view.entries }
      : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

export type ScansOutcome =
  { kind: "ok"; devices: DeviceScanView[] } | { kind: "signed_out" } | { kind: "unreachable" };

/** Every live paired PC with the time and size of its last scan (`null` when never scanned). */
export async function loadScans(): Promise<ScansOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(SCANS_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    const body: unknown = res.ok ? await res.json() : null;
    const view = body as LibraryScansView | null;
    return view && Array.isArray(view.devices)
      ? { kind: "ok", devices: view.devices }
      : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

const hasControl = (text: string) => /[\u0000-\u001f\u007f-\u009f]/.test(text);
const plain = (text: string, max: number) => text.length <= max && !hasControl(text);

/** The tags a text field holds: comma separated, trimmed, empty ones dropped. */
export function parseTags(text: string): string[] {
  const tags: string[] = [];
  for (const part of text.split(",")) {
    const tag = part.trim();
    if (tag && !tags.some((x) => x.toLowerCase() === tag.toLowerCase())) tags.push(tag);
  }
  return tags;
}

/** Tags as the text of an input. */
export const tagsText = (tags: string[]) => tags.join(", ");

/** Whether `path` is a full Windows path (`C:\…`) without a wildcard or a placeholder. */
export const isFullPath = (path: string) =>
  /^[A-Za-z]:\\.+/.test(path) && !/[*?{}]/.test(path) && !hasControl(path);

/** Whether `uri` starts with a scheme of two or more characters, then something. */
export const isLaunchUri = (uri: string) =>
  /^[A-Za-z][A-Za-z0-9+.-]+:.+/.test(uri) && !hasControl(uri);

export type EntryForm = {
  kind: string;
  name: string;
  launcher: string;
  executablePath: string;
  launchUri: string;
  launchArgs: string;
  category: string;
  tags: string;
  ageRating: string;
  catalogId: string;
  enabled: boolean;
};

/**
 * The form's errors, by the field names the Hub uses (`name`, `executable_path`, …), each a
 * catalogue message. A linked entry's name, launcher and age rating are the catalogue's, so they
 * are not checked here.
 */
export function checkForm(form: EntryForm, linked: boolean): Record<string, MessageKey> {
  const bad: Record<string, MessageKey> = {};
  const name = form.name.trim();
  const path = form.executablePath.trim();
  const uri = form.launchUri.trim();
  const tags = parseTags(form.tags);
  if (!KINDS.includes(form.kind as (typeof KINDS)[number]))
    bad.entry_kind = "hub.games.invalid.kind";
  if (!linked && (name.length === 0 || !plain(name, NAME_MAX))) bad.name = "hub.games.invalid.name";
  if (!linked && !LAUNCHERS.includes(form.launcher as (typeof LAUNCHERS)[number])) {
    bad.launcher = "hub.games.invalid.launcher";
  }
  if (
    (path.length === 0 && uri.length === 0) ||
    (path && (path.length > PATH_MAX || !isFullPath(path)))
  ) {
    bad.executable_path = "hub.games.invalid.executable_path";
  }
  if (uri && (uri.length > URI_MAX || !isLaunchUri(uri)))
    bad.launch_uri = "hub.games.invalid.launch_uri";
  if (!plain(form.launchArgs.trim(), ARGS_MAX)) bad.launch_args = "hub.games.invalid.launch_args";
  if (!plain(form.category.trim(), CATEGORY_MAX)) bad.category = "hub.games.invalid.category";
  if (tags.length > TAGS_MAX || tags.some((x) => !plain(x, TAG_MAX)))
    bad.tags = "hub.games.invalid.tags";
  if (!linked && !plain(form.ageRating.trim(), AGE_MAX))
    bad.age_rating = "hub.games.invalid.age_rating";
  if (!plain(form.catalogId.trim(), CATALOG_ID_MAX))
    bad.catalog_id = "hub.games.invalid.catalog_id";
  return bad;
}
