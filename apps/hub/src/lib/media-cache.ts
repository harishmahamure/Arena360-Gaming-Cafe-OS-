import { t } from "@arena/ui";

/** The branch setting that caps the Hub's game media cache, in GB (Story 6.2). */
export const KEY_MEDIA_CAP_GB = "system.media_cap_gb";
export const MEDIA_CAP_MIN_GB = 1;
export const MEDIA_CAP_MAX_GB = 10000;

const KIB = 1024;
const MIB = KIB * 1024;
const GIB = MIB * 1024;

/** A size as the screen shows it: GB with one decimal from 1 GB up, else MB, KB or bytes. */
export function formatBytes(bytes: number): string {
  if (bytes >= GIB) {
    const gb = bytes / GIB;
    return t("hub.settings.system.media.size_gb", { n: Number.isInteger(gb) ? gb : gb.toFixed(1) });
  }
  if (bytes >= MIB) return t("hub.settings.system.media.size_mb", { n: Math.round(bytes / MIB) });
  if (bytes >= KIB) return t("hub.settings.system.media.size_kb", { n: Math.round(bytes / KIB) });
  return t("hub.settings.system.media.size_b", { n: bytes });
}

/** The cap a text field holds: a whole number of GB from 1 to 10000, else `null`. */
export function parseCapGb(text: string): number | null {
  const s = text.trim();
  if (!/^\d{1,5}$/.test(s)) return null;
  const n = Number(s);
  return n >= MEDIA_CAP_MIN_GB && n <= MEDIA_CAP_MAX_GB ? n : null;
}
