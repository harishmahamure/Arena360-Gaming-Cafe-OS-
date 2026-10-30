// A product picture, made small enough in the browser for the Hub to keep it (Story 8.24): a JPEG
// of at most 512 px on a side and 200 KB, as the Hub's own product form makes it.

export const MAX_IMAGE_PX = 512;
export const MAX_IMAGE_BYTES = 200 * 1024;
const QUALITIES = [0.85, 0.7, 0.55, 0.4, 0.25];

/** The picture as a JPEG data URL within the limits, or `null` when it is not a picture or cannot fit. */
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
