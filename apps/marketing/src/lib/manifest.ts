// The release manifest the pipeline writes to the signed-installer prefix of R2 (AD-32), and the
// rules for reading it. Pure, with no imports, so the script tests can load it too.

/** The products that ship as a signed file from our site. The Android Hub ships through Play. */
export const FILE_PRODUCTS = ["hub-windows", "kiosk-windows", "pi-image"] as const;
export type FileProduct = (typeof FILE_PRODUCTS)[number];

/** One line of `releases/manifest.json`. */
export interface Release {
  product: FileProduct;
  channel: "stable";
  version: string;
  /** The installer's file name, under `releases/<product>/<version>/`. */
  file: string;
  /** Bytes. */
  size: number;
  /** Lower-case hex SHA-256 of the file. */
  sha256: string;
  signature_url: string;
  /** ISO 8601. */
  published_at: string;
}

/** A name that is safe to put in a URL path and in a PowerShell command. */
const SAFE_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;
const SAFE_VERSION = /^[0-9A-Za-z][0-9A-Za-z.+-]{0,63}$/;
const SHA256 = /^[0-9a-f]{64}$/;

/** Reads one manifest line, or `null` when it is not a complete stable release of a known file. */
export function parseRelease(value: unknown): Release | null {
  if (typeof value !== "object" || value === null) return null;
  const r = value as Record<string, unknown>;
  const { product, channel, version, file, size, sha256, signature_url, published_at } = r;
  if (!FILE_PRODUCTS.includes(product as FileProduct)) return null;
  if (channel !== "stable") return null;
  if (typeof version !== "string" || !SAFE_VERSION.test(version)) return null;
  if (typeof file !== "string" || !SAFE_NAME.test(file)) return null;
  if (typeof size !== "number" || !Number.isSafeInteger(size) || size <= 0) return null;
  if (typeof sha256 !== "string" || !SHA256.test(sha256)) return null; // no checksum, no link
  if (typeof signature_url !== "string" || !signature_url.startsWith("https://")) return null;
  if (typeof published_at !== "string" || Number.isNaN(Date.parse(published_at))) return null;
  return {
    product: product as FileProduct,
    channel: "stable",
    version,
    file,
    size,
    sha256,
    signature_url,
    published_at,
  };
}

/**
 * The stable releases of a manifest document, newest per product. Anything that is not a complete
 * line is dropped, so no row ever links to a file whose checksum it cannot show. Throws when the
 * document is not a manifest at all.
 */
export function parseManifest(doc: unknown): Release[] {
  const lines =
    typeof doc === "object" &&
    doc !== null &&
    Array.isArray((doc as { releases?: unknown }).releases)
      ? (doc as { releases: unknown[] }).releases
      : null;
  if (lines === null) throw new Error("not a release manifest"); // i18n-ignore
  const newest = new Map<FileProduct, Release>();
  for (const line of lines) {
    const release = parseRelease(line);
    if (release === null) continue;
    const kept = newest.get(release.product);
    if (kept === undefined || Date.parse(release.published_at) > Date.parse(kept.published_at)) {
      newest.set(release.product, release);
    }
  }
  return FILE_PRODUCTS.flatMap((product) => newest.get(product) ?? []);
}
