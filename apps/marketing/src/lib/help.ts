// The Help centre's rules: article categories, the static search, the links between articles and the
// hardware compatibility data file. Pure, with no imports, so the script tests can load it too.
// Words live in the message catalogue (`site.help.*`); articles are Markdown in `src/content/help`.

/** The two article categories. Hardware compatibility is the third category and is a list, not articles. */
export const ARTICLE_CATEGORIES = ["setup", "troubleshooting"] as const;
export type ArticleCategory = (typeof ARTICLE_CATEGORIES)[number];

/** The stable anchor the Downloads page links to. */
export const HARDWARE_ANCHOR = "hardware-compatibility";

/** What search and the link check need from an article. */
export interface HelpArticle {
  /** The stable slug: the Markdown file name. */
  slug: string;
  title: string;
  summary: string;
  category: ArticleCategory;
  /** The Markdown body, for the link check. */
  body?: string;
}

/** The slug in a file's collection id (`install-the-hub-on-windows`), or `null` when it is not stable. */
export const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/**
 * Links from one article to another, written `/help/<slug>` (optionally `#anchor`) in Markdown.
 * Returns each one whose slug no article has, as `from -> /help/<slug>`.
 */
export function brokenLinks(articles: HelpArticle[]): string[] {
  const slugs = new Set(articles.map((a) => a.slug));
  const broken: string[] = [];
  for (const article of articles) {
    for (const match of (article.body ?? "").matchAll(/\]\(\/help\/([^)#?\s]*)[^)]*\)/g)) {
      const target = match[1] ?? "";
      if (!slugs.has(target)) broken.push(`${article.slug} -> /help/${target}`); // i18n-ignore
    }
  }
  return broken;
}

/** Throws (so the build fails) when a link points to a missing slug or a slug is not stable. */
export function assertHelp(articles: HelpArticle[]): void {
  const bad = articles.filter((a) => !SLUG.test(a.slug)).map((a) => a.slug);
  if (bad.length > 0)
    throw new Error(`Help article slugs must be lower-case words: ${bad.join(", ")}`); // i18n-ignore
  const broken = brokenLinks(articles);
  if (broken.length > 0) throw new Error(`Help links to a missing article: ${broken.join("; ")}`); // i18n-ignore
}

/** One entry of the search index built at deploy: no body, so it stays small. */
export interface SearchEntry {
  slug: string;
  title: string;
  summary: string;
  category: ArticleCategory;
}

export const searchIndex = (articles: HelpArticle[]): SearchEntry[] =>
  articles.map(({ slug, title, summary, category }) => ({ slug, title, summary, category }));

const words = (text: string) =>
  text
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);

/**
 * Articles whose title or summary holds every word of `term` (a word may be the start of a longer
 * one), title matches first. Empty for an empty term. Runs in the browser with no server call.
 */
export function searchHelp(index: SearchEntry[], term: string): SearchEntry[] {
  const wanted = words(term);
  if (wanted.length === 0) return [];
  const scored: { entry: SearchEntry; score: number }[] = [];
  for (const entry of index) {
    const inTitle = words(entry.title);
    const inSummary = words(entry.summary);
    let score = 0;
    let all = true;
    for (const w of wanted) {
      const title = inTitle.some((x) => x.startsWith(w));
      const summary = inSummary.some((x) => x.startsWith(w));
      if (!title && !summary) {
        all = false;
        break;
      }
      score += title ? 2 : 1;
    }
    if (all) scored.push({ entry, score });
  }
  return scored.sort((a, b) => b.score - a.score).map((s) => s.entry);
}

// ---- Hardware compatibility -------------------------------------------------------------------

/** Device categories in the data file. New ones are added as rows, never as a page change. */
export const HARDWARE_CATEGORIES = ["printer", "drawer"] as const;
export type HardwareCategory = (typeof HARDWARE_CATEGORIES)[number];

export const CONNECTIONS = ["usb", "lan", "bluetooth"] as const;
export type Connection = (typeof CONNECTIONS)[number];

export const RESULTS = ["works", "partial", "fails"] as const;
export type HardwareResult = (typeof RESULTS)[number];

/** The Hubs a device has been printed from on reference hardware. */
export const REFERENCE_HUBS = ["windows", "android"] as const;

export interface HardwareRow {
  category: HardwareCategory;
  brand: string;
  model: string;
  connection: Connection;
  /** Receipt paper width in mm; `null` for a drawer. */
  paper_mm: 58 | 80 | null;
  result: HardwareResult;
  /** ISO date the result was checked. */
  checked: string;
  /** The Hubs the Arena 360 team has printed from on reference hardware. */
  tested_on: (typeof REFERENCE_HUBS)[number][];
}

export interface HardwareFile {
  /** The file's format version. */
  version: 1;
  rows: HardwareRow[];
}

const ISO_DAY = /^\d{4}-\d{2}-\d{2}$/;
const oneOf = <T extends readonly string[]>(list: T, v: unknown): v is T[number] =>
  typeof v === "string" && (list as readonly string[]).includes(v);

/** Reads one row, or throws naming what is wrong (a bad data file fails the build). */
export function parseHardwareRow(value: unknown, at: number): HardwareRow {
  const fail = (what: string): never => {
    throw new Error(`hardware.json row ${at}: ${what}`); // i18n-ignore
  };
  const r = (value ?? {}) as Record<string, unknown>;
  if (!oneOf(HARDWARE_CATEGORIES, r.category)) return fail("category must be printer or drawer"); // i18n-ignore
  if (typeof r.brand !== "string" || r.brand.trim() === "") return fail("brand is required"); // i18n-ignore
  if (typeof r.model !== "string" || r.model.trim() === "") return fail("model is required"); // i18n-ignore
  if (!oneOf(CONNECTIONS, r.connection)) return fail("connection must be usb, lan or bluetooth"); // i18n-ignore
  if (!oneOf(RESULTS, r.result)) return fail("result must be works, partial or fails"); // i18n-ignore
  if (typeof r.checked !== "string" || !ISO_DAY.test(r.checked))
    return fail("checked must be YYYY-MM-DD"); // i18n-ignore
  const paper = r.paper_mm;
  if (r.category === "printer" ? paper !== 58 && paper !== 80 : paper !== null)
    return fail("paper_mm is 58 or 80 for a printer and null for a drawer"); // i18n-ignore
  const tested = r.tested_on;
  if (!Array.isArray(tested) || !tested.every((h) => oneOf(REFERENCE_HUBS, h)))
    return fail("tested_on lists windows and/or android"); // i18n-ignore
  return {
    category: r.category,
    brand: r.brand.trim(),
    model: r.model.trim(),
    connection: r.connection,
    paper_mm: paper as 58 | 80 | null,
    result: r.result,
    checked: r.checked,
    tested_on: [...new Set(tested)] as HardwareRow["tested_on"],
  };
}

export function parseHardware(value: unknown): HardwareFile {
  const file = (value ?? {}) as { version?: unknown; rows?: unknown };
  if (file.version !== 1) throw new Error("hardware.json: version must be 1"); // i18n-ignore
  if (!Array.isArray(file.rows)) throw new Error("hardware.json: rows must be a list"); // i18n-ignore
  const rows = file.rows.map((row, i) => parseHardwareRow(row, i));
  // The table keys each row by these fields, so a repeat would break it in the browser.
  const seen = new Set<string>();
  rows.forEach((r, i) => {
    const id = [r.category, r.brand, r.model, r.connection, r.paper_mm].join("|");
    if (seen.has(id)) throw new Error(`hardware.json row ${i}: duplicate of an earlier row`); // i18n-ignore
    seen.add(id);
  });
  return { version: 1, rows };
}

/** "Tested device": printed from on reference hardware with both a Windows and an Android Hub. */
export const isTested = (row: HardwareRow) =>
  REFERENCE_HUBS.every((h) => row.tested_on.includes(h));

/** The brands in the rows, sorted, once each. */
export const brandsOf = (rows: HardwareRow[]) =>
  [...new Set(rows.map((r) => r.brand))].sort((a, b) => a.localeCompare(b));

/** Rows of `brand` and `connection`; an empty filter keeps every row. */
export const filterHardware = (rows: HardwareRow[], brand: string, connection: string) =>
  rows.filter((r) => (!brand || r.brand === brand) && (!connection || r.connection === connection));
