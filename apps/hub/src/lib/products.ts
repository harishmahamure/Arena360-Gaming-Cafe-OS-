import type { Product } from "@arena/ui/generated";
import { authHeaders, getToken, setToken } from "./auth";

export const PRODUCTS_URL = "http://127.0.0.1:7409/ui/products";
export const MAX_IMAGE_PX = 512;
export const MAX_IMAGE_BYTES = 200 * 1024;
const QUALITIES = [0.85, 0.7, 0.55, 0.4, 0.25];

export type ProductsOutcome =
  { kind: "ok"; products: Product[] } | { kind: "signed_out" } | { kind: "unreachable" };

export async function loadProducts(): Promise<ProductsOutcome> {
  const token = getToken();
  if (!token) return { kind: "signed_out" };
  try {
    const res = await fetch(PRODUCTS_URL, {
      headers: authHeaders(token),
      signal: AbortSignal.timeout(5000),
      cache: "no-store",
    });
    if (res.status === 401) {
      setToken(null);
      return { kind: "signed_out" };
    }
    const body: unknown = res.ok ? await res.json() : null;
    return Array.isArray(body)
      ? { kind: "ok", products: body as Product[] }
      : { kind: "unreachable" };
  } catch {
    return { kind: "unreachable" };
  }
}

export function parsePrice(text: string, exponent: number): number | null {
  const m = /^(\d{1,12})(?:\.(\d*))?$/.exec(text.trim());
  if (!m || (m[2] ?? "").length > exponent) return null;
  return Number(m[1]) * 10 ** exponent + Number((m[2] ?? "").padEnd(exponent, "0") || 0);
}

export const priceText = (minor: number, exponent: number) =>
  (minor / 10 ** exponent).toFixed(exponent);

export async function shrinkImage(file: File): Promise<string | null> {
  if (!file.type.startsWith("image/")) return null;
  try {
    const bitmap = await createImageBitmap(file);
    const scale = Math.min(1, MAX_IMAGE_PX / Math.max(bitmap.width, bitmap.height));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(bitmap.width * scale));
    canvas.height = Math.max(1, Math.round(bitmap.height * scale));
    const ctx = canvas.getContext("2d");
    if (!ctx) return null;
    ctx.fillStyle = "white";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    bitmap.close();
    for (const q of QUALITIES) {
      const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, "image/jpeg", q));
      if (blob && blob.size <= MAX_IMAGE_BYTES) {
        return await new Promise<string | null>((resolve) => {
          const reader = new FileReader();
          reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : null);
          reader.onerror = () => resolve(null);
          reader.readAsDataURL(blob);
        });
      }
    }
    return null;
  } catch {
    return null;
  }
}
