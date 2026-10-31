import type { APIRoute } from "astro";
import { isStaging, siteOrigin } from "../lib/site";

// A machine-read file, not site copy. Production welcomes crawlers and names the sitemap; staging
// asks them to stay away.
const STAGING = "User-agent: *\nDisallow: /\n"; // i18n-ignore
const PRODUCTION = (origin: string) =>
  `User-agent: *\nAllow: /\n\nSitemap: ${origin}/sitemap.xml\n`; // i18n-ignore
const TEXT = "text/plain; charset=utf-8"; // i18n-ignore

export const GET: APIRoute = () => {
  const body = isStaging() ? STAGING : PRODUCTION(siteOrigin());
  return new Response(body, { headers: { "Content-Type": TEXT } });
};
