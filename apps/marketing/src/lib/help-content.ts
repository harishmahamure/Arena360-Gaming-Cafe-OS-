import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { getCollection } from "astro:content";
import { assertHelp, parseHardware, type HardwareFile, type HelpArticle } from "./help";

// Reads the Help centre at build time. Pages import from here, so a link to a missing slug or a bad
// hardware row fails every build that touches the Help centre.

export interface HelpEntry extends HelpArticle {
  reviewed: Date;
}

/** Every article, by title, after the link check. */
export async function helpArticles(): Promise<HelpEntry[]> {
  const entries = await getCollection("help");
  const articles = entries
    .map((entry) => ({
      slug: entry.id,
      title: entry.data.title,
      summary: entry.data.summary,
      category: entry.data.category,
      reviewed: entry.data.reviewed,
      body: entry.body ?? "",
    }))
    .sort((a, b) => a.title.localeCompare(b.title));
  assertHelp(articles);
  return articles;
}

/**
 * The versioned hardware data file. `PUBLIC_HARDWARE_DATA` points a build at another file (tests).
 */
export function hardwareFile(): HardwareFile {
  const path =
    import.meta.env.PUBLIC_HARDWARE_DATA || resolve(process.cwd(), "src", "data", "hardware.json");
  return parseHardware(JSON.parse(readFileSync(path, "utf8")));
}
