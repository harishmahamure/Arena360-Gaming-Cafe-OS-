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
  const path = route.replace(/(^|\/)index$/, "");
  return "/" + path;
}
