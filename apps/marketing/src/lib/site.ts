// Where the site lives. `astro build` is production (arena360.cloud); `astro build --mode staging`
// is staging (staging.arena360.cloud), which asks search engines to stay away.

export const ORIGINS = {
  production: "https://arena360.cloud",
  staging: "https://staging.arena360.cloud",
} as const;

/** Where "Get started" hands off to: the onboarding web app of the same environment. */
export const START_URLS = {
  production: "https://start.arena360.cloud",
  staging: "https://start.staging.arena360.cloud",
} as const;

export const isStaging = () => import.meta.env.MODE === "staging";

export const siteOrigin = () => (isStaging() ? ORIGINS.staging : ORIGINS.production);

export const startUrl = () => (isStaging() ? START_URLS.staging : START_URLS.production);

/** The canonical URL of a path: no trailing slash except for Home. */
export function canonicalUrl(pathname: string): string {
  const path = pathname.length > 1 ? pathname.replace(/\/+$/, "") : pathname;
  return siteOrigin() + (path || "/");
}

/**
 * The public path a page file serves: `index.astro` is `/`, `pricing.astro` and
 * `pricing/index.astro` are `/pricing`. Dynamic routes and the 404 page have no fixed path.
 */
export function routeOf(file: string): string | null {
  const match = /(?:^|\/)pages\/(.+)\.astro$/.exec(file);
  const route = match?.[1];
  if (route === undefined || route === "404" || route.includes("[")) return null;
  if (route.split("/").some((part) => part.startsWith("_"))) return null; // Astro does not route these
  const path = route.replace(/(^|\/)index$/, "");
  return "/" + path;
}

/**
 * The hero's background video, in the order the browser should try them. Empty until a muted clip
 * with no audio track is cleared for use; the still cover and then solid `bg` show meanwhile.
 */
export const HERO_VIDEO: readonly { src: string; type: string }[] = [];

/**
 * Where the marketing media (demo videos, posters, captions, transcripts) is served from: the
 * shared `marketing/` prefix of the R2 bucket, in front of Cloudflare. One origin for production
 * and staging, so a staging build previews the same media.
 */
export const MEDIA_ORIGIN = "https://media.arena360.cloud";

/** The URL of a file under the shared `marketing/` prefix. */
export const mediaUrl = (path: string) => `${MEDIA_ORIGIN}/marketing/${path}`; // i18n-ignore

/**
 * Where the signed installers and the release manifest are served from: the `releases/` prefix of
 * the R2 bucket (AD-32), in front of Cloudflare. One origin for production and staging.
 */
export const RELEASES_ORIGIN = "https://downloads.arena360.cloud";

/** The release manifest the pipeline writes next to the installers. */
export const RELEASES_MANIFEST_URL = `${RELEASES_ORIGIN}/releases/manifest.json`; // i18n-ignore

/** The Android Hub's page on Google Play (package `in.arena360.hub`, never an APK). */
export const PLAY_URL = "https://play.google.com/store/apps/details?id=in.arena360.hub";
