#!/usr/bin/env node
// The release manifest the pipeline writes next to the signed installers (Story 11.5, AD-32).
//
//   add --manifest <file> --product <id> --version <v> --file <installer> \
//       --signature-url <https url> [--published-at <iso>]
//                              hashes the installer and adds (or replaces) its line in the
//                              manifest, creating the file if it is missing
//
// The Downloads page reads `releases/manifest.json` at build time and again in the browser:
// `{ "releases": [ { product, channel: "stable", version, file, size, sha256, signature_url,
// published_at } ] }`. Everything that decides something is a pure function exported for
// `release-manifest.test.mjs`. Local files only; uploading to R2 is the pipeline's next step.

import { createHash } from "node:crypto";
import { existsSync, readFileSync, realpathSync, statSync, writeFileSync } from "node:fs";
import { basename } from "node:path";
import { fileURLToPath } from "node:url";

export const PRODUCTS = ["hub-windows", "kiosk-windows", "pi-image"];
export const CHANNEL = "stable";

/** The SHA-256 (lower-case hex) of a file. */
export const sha256File = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");

/** One manifest line for an installer on disk. Throws on anything the page would drop. */
export function releaseLine({ product, version, path, signatureUrl, publishedAt }) {
  if (!PRODUCTS.includes(product)) throw new Error(`unknown product "${product}"`);
  if (!/^[0-9A-Za-z][0-9A-Za-z.+-]{0,63}$/.test(version ?? "")) {
    throw new Error(`bad version "${version}"`);
  }
  const file = basename(path);
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(file)) throw new Error(`bad file name "${file}"`);
  if (!String(signatureUrl).startsWith("https://")) throw new Error("signature URL must be https");
  const when = publishedAt ?? new Date().toISOString();
  if (Number.isNaN(Date.parse(when))) throw new Error(`bad published_at "${when}"`);
  const size = statSync(path).size;
  if (size <= 0) throw new Error(`${file} is empty`);
  return {
    product,
    channel: CHANNEL,
    version,
    file,
    size,
    sha256: sha256File(path),
    signature_url: signatureUrl,
    published_at: when,
  };
}

/** The manifest with `line` added; a line for the same product and version is replaced. */
export function addRelease(manifest, line) {
  const kept = (manifest?.releases ?? []).filter(
    (r) =>
      !(r.product === line.product && r.channel === line.channel && r.version === line.version),
  );
  const releases = [...kept, line].sort(
    (a, b) =>
      a.product.localeCompare(b.product) || Date.parse(b.published_at) - Date.parse(a.published_at),
  );
  return { releases };
}

const arg = (args, name) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : undefined;
};

export function main(args) {
  if (args[0] !== "add") {
    console.error(
      "usage: release-manifest.mjs add --manifest <file> --product <id> --version <v> --file <installer> --signature-url <url> [--published-at <iso>]",
    );
    return 64;
  }
  const manifestPath = arg(args, "manifest");
  const path = arg(args, "file");
  if (!manifestPath || !path) throw new Error("--manifest and --file are required");
  const line = releaseLine({
    product: arg(args, "product"),
    version: arg(args, "version"),
    path,
    signatureUrl: arg(args, "signature-url"),
    publishedAt: arg(args, "published-at"),
  });
  const existing = existsSync(manifestPath) ? JSON.parse(readFileSync(manifestPath, "utf8")) : null;
  writeFileSync(manifestPath, JSON.stringify(addRelease(existing, line), null, 2) + "\n");
  console.log(`${line.product} ${line.version}: ${line.sha256}`);
  return 0;
}

if (
  process.argv[1] &&
  realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))
) {
  try {
    process.exit(main(process.argv.slice(2)));
  } catch (e) {
    console.error(`::error::${e.message}`);
    process.exit(1);
  }
}
