// The branch logo (Story 6.7): a PNG or SVG stored as the `kiosk.logo` setting, a data URI of at
// most 512 px and 200 KB. The Hub checks the same rules (`arena_proto::logo_problem`); this is
// the friendly front half that resizes a PNG and refuses an SVG with active content before it is
// sent.

export const KEY_KIOSK_LOGO = "kiosk.logo";
export const MAX_LOGO_PX = 512;
export const MAX_LOGO_BYTES = 200 * 1024;
export const PNG_PREFIX = "data:image/png;base64,";
export const SVG_PREFIX = "data:image/svg+xml;base64,";

export type LogoOutcome = { kind: "ok"; uri: string } | { kind: "type" | "read" | "size" | "svg" };

const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/** The data URI of `bytes` (base64 in chunks, so a large file never overflows the stack). */
export function toDataUri(prefix: string, bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return prefix + btoa(binary);
}

/** A PNG's width and height from its signature and IHDR, or `null` when it is not a PNG. */
export function pngSize(bytes: Uint8Array): { width: number; height: number } | null {
  if (bytes.length < 33 || !PNG_SIGNATURE.every((b, i) => bytes[i] === b)) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const ihdr = String.fromCharCode(...bytes.subarray(12, 16));
  if (view.getUint32(8) !== 13 || ihdr !== "IHDR") return null;
  const width = view.getUint32(16);
  const height = view.getUint32(20);
  return width > 0 && height > 0 ? { width, height } : null;
}

const PROLOG = /^﻿?\s*(?:<\?[\s\S]*?\?>\s*|<!--[\s\S]*?-->\s*|<!doctype[^>[]*>\s*)*/i;
const ACTIVE = [
  "<script",
  "<foreignobject",
  "<iframe",
  "<embed",
  "<object",
  "<!entity",
  "<!doctype",
  "javascript:",
  "@import",
];
const HANDLER = /[\s"'/]on[a-z]+\s*=/;
const HREF = /href\s*=\s*(["']?)\s*([^"'\s>]*)/g;
const URL_REF = /url\(\s*["']?\s*([^"')\s]*)/g;

/**
 * Why an SVG must not be a logo: it is not an SVG, it has active content (a script, a handler,
 * `javascript:`), or it points outside itself (`href` or `url(` to anything but `#id`).
 * `null` when it is fine.
 */
export function svgProblem(text: string): "invalid" | "active" | "external" | null {
  const body = text.replace(PROLOG, "");
  const lower = body.toLowerCase();
  if (!/^<svg[\s/>]/.test(lower)) return "invalid";
  if (ACTIVE.some((a) => lower.includes(a)) || HANDLER.test(lower)) return "active";
  for (const m of lower.matchAll(HREF)) if (!m[2].startsWith("#")) return "external";
  for (const m of lower.matchAll(URL_REF)) if (!m[1].startsWith("#")) return "external";
  return null;
}

async function resizedPng(file: File): Promise<Uint8Array | null> {
  const bitmap = await createImageBitmap(file);
  const scale = Math.min(1, MAX_LOGO_PX / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(bitmap.width * scale));
  canvas.height = Math.max(1, Math.round(bitmap.height * scale));
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  // The logo keeps its transparency: no background is painted.
  ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, "image/png"));
  return blob ? new Uint8Array(await blob.arrayBuffer()) : null;
}

/**
 * Turns a chosen file into a logo data URI. A PNG within 512 px and 200 KB is kept as it is;
 * a larger one is resized to at most 512 px on a canvas, and one still over 200 KB is refused.
 * An SVG is read as text and must pass [`svgProblem`] and the size limit.
 */
export async function prepareLogo(file: File): Promise<LogoOutcome> {
  const isSvg = file.type === "image/svg+xml" || /\.svg$/i.test(file.name);
  const isPng = file.type === "image/png" || /\.png$/i.test(file.name);
  if (!isSvg && !isPng) return { kind: "type" };
  try {
    if (isSvg) {
      if (file.size > MAX_LOGO_BYTES) return { kind: "size" };
      const bytes = new Uint8Array(await file.arrayBuffer());
      let text: string;
      try {
        text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
      } catch {
        return { kind: "svg" };
      }
      return svgProblem(text) === null
        ? { kind: "ok", uri: toDataUri(SVG_PREFIX, bytes) }
        : { kind: "svg" };
    }
    const original = new Uint8Array(await file.arrayBuffer());
    const size = pngSize(original);
    if (size === null) return { kind: "read" };
    let bytes: Uint8Array | null = original;
    if (size.width > MAX_LOGO_PX || size.height > MAX_LOGO_PX || original.length > MAX_LOGO_BYTES) {
      bytes = await resizedPng(file);
      if (bytes === null) return { kind: "read" };
    }
    return bytes.length <= MAX_LOGO_BYTES
      ? { kind: "ok", uri: toDataUri(PNG_PREFIX, bytes) }
      : { kind: "size" };
  } catch {
    return { kind: "read" };
  }
}
