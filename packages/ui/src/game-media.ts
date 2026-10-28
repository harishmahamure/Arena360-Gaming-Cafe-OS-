// Rules behind the `GameMedia` component: which media URLs are allowed, which step of the
// fallback chain shows, and which one video plays when several components ask.

/** The Hub's media port (Story 6.2); media is only ever fetched from the Hub by content hash. */
export const HUB_MEDIA_PORT = "7401";

const MEDIA_PATH = /^\/media\/[0-9a-f]{64}$/;

/**
 * True for a Hub media URL: `/media/<sha256>`, either relative to the page or absolute on the
 * Hub's media port. Anything else (an internet URL, a data URL, another path) is refused.
 */
export function isHubMediaUrl(url: string | null | undefined): url is string {
  if (!url) return false;
  if (url.startsWith("/")) return MEDIA_PATH.test(url);
  try {
    const parsed = new URL(url);
    return (
      (parsed.protocol === "https:" || parsed.protocol === "http:") &&
      parsed.port === HUB_MEDIA_PORT &&
      !parsed.search &&
      !parsed.hash &&
      MEDIA_PATH.test(parsed.pathname)
    );
  } catch {
    return false;
  }
}

export type MediaStage = "video" | "cover" | "icon" | "solid";

export interface MediaSources {
  video?: string | null;
  cover?: string | null;
  icon?: string | null;
  /** A game is running: the video is unloaded and the cover shows. */
  gameRunning?: boolean;
  /** The high-contrast theme: no media at all. */
  highContrast?: boolean;
  /** URLs that failed to load or decode; their step is skipped. */
  failed?: readonly string[];
}

/** The first usable step of video → cover → icon → solid. */
export function mediaStage(s: MediaSources): MediaStage {
  if (s.highContrast) return "solid";
  const usable = (url: string | null | undefined) => isHubMediaUrl(url) && !s.failed?.includes(url);
  if (!s.gameRunning && usable(s.video)) return "video";
  if (usable(s.cover)) return "cover";
  if (usable(s.icon)) return "icon";
  return "solid";
}

// One video at a time: the most recent claim plays, and the one before it resumes when that
// claim is released.
const claims: string[] = [];
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((fn) => fn());

/** Asks to play a video; the latest claim wins. Returns the release function. */
export function claimVideo(id: string): () => void {
  claims.push(id);
  notify();
  return () => {
    const at = claims.lastIndexOf(id);
    if (at >= 0) claims.splice(at, 1);
    notify();
  };
}

/** True while `id` holds the newest claim. */
export function isActiveVideo(id: string): boolean {
  return claims[claims.length - 1] === id;
}

/** Calls `fn` whenever a claim is made or released. Returns the unsubscribe function. */
export function onVideoClaims(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

let sequence = 0;
/** A new id for one component's video claim. */
export const nextClaimId = (): string => String(++sequence);
