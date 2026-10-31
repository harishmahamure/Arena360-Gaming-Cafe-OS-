import type { MessageKey } from "@arena/ui/messages";
import { parseManifest, type FileProduct, type Release } from "./manifest";
import { PLAY_URL, RELEASES_MANIFEST_URL, RELEASES_ORIGIN } from "./site";

export {
  FILE_PRODUCTS,
  parseManifest,
  parseRelease,
  type FileProduct,
  type Release,
} from "./manifest";

// The Downloads page's data: the release manifest the pipeline writes to the signed-installer
// prefix of R2 (AD-32), read at build time and again in the browser. Words live in the message
// catalogue (`site.downloads.*`); this file is the structure, the manifest rules and the formats.

export interface Download {
  id: FileProduct | "hub-android";
  /** Where it installs: the row's name and what it is for. */
  name: MessageKey;
  about: MessageKey;
  kind: "file" | "play";
}

export const DOWNLOADS: Download[] = [
  {
    id: "hub-windows",
    name: "site.downloads.hub.name",
    about: "site.downloads.hub.about",
    kind: "file",
  },
  {
    id: "kiosk-windows",
    name: "site.downloads.kiosk.name",
    about: "site.downloads.kiosk.about",
    kind: "file",
  },
  {
    id: "hub-android",
    name: "site.downloads.android.name",
    about: "site.downloads.android.about",
    kind: "play",
  },
  {
    id: "pi-image",
    name: "site.downloads.pi.name",
    about: "site.downloads.pi.about",
    kind: "file",
  },
];

/** Reads the manifest, or throws when it cannot be fetched or is not a manifest. */
export async function fetchManifest(
  url: string = RELEASES_MANIFEST_URL,
  timeoutMs = 8000,
): Promise<Release[]> {
  const response = await fetch(url, {
    cache: "no-store",
    signal: AbortSignal.timeout(timeoutMs),
  });
  if (!response.ok) throw new Error(`manifest ${response.status}`); // i18n-ignore
  return parseManifest(await response.json());
}

/** The signed installer's address: `releases/<product>/<version>/<file>`. */
export const installerUrl = (release: Release) =>
  `${RELEASES_ORIGIN}/releases/${release.product}/${release.version}/${release.file}`; // i18n-ignore

export const playUrl = () => PLAY_URL;

/** The PowerShell line that prints a downloaded file's SHA-256, to compare with the one shown. */
export const verifyCommand = (release: Release) =>
  `Get-FileHash .\\${release.file} -Algorithm SHA256`; // i18n-ignore

/** Megabytes with one decimal: `84.3`. */
export const megabytes = (bytes: number) => (bytes / (1024 * 1024)).toFixed(1);

/** `31 Oct 2026`, in the café's own time (India), the same on the server and in the browser. */
export const releaseDate = (iso: string) =>
  new Intl.DateTimeFormat("en-IN", {
    day: "numeric",
    month: "short",
    year: "numeric",
    timeZone: "Asia/Kolkata",
  }).format(new Date(iso));
