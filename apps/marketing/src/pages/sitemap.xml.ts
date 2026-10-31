import type { APIRoute } from "astro";
import { helpArticles } from "../lib/help-content";
import { LEGAL_SLUGS } from "../lib/legal";
import { canonicalUrl, routeOf } from "../lib/site";

// Every static page under `src/pages`, every Help article and every Legal page, listed by its canonical URL.
export const GET: APIRoute = async () => {
  const paths = Object.keys(import.meta.glob("./**/*.astro"))
    .map((file) => routeOf("pages/" + file.replace(/^\.\//, "")))
    .filter((path): path is string => path !== null);
  // Dynamic routes have no fixed path: each Help article is listed by its slug.
  for (const article of await helpArticles()) paths.push(`/help/${article.slug}`);
  for (const slug of LEGAL_SLUGS) paths.push(`/legal/${slug}`);
  paths.sort();
  const urls = paths.map((path) => `  <url><loc>${canonicalUrl(path)}</loc></url>`).join("\n");
  const body = `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls}\n</urlset>\n`;
  return new Response(body, { headers: { "Content-Type": "application/xml; charset=utf-8" } }); // i18n-ignore
};
