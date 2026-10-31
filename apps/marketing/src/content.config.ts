import { glob } from "astro/loaders";
import { defineCollection } from "astro:content";
import { z } from "astro/zod";
import { ARTICLE_CATEGORIES } from "./lib/help";

// Help centre articles: Markdown files in `src/content/help`. The file name is the stable slug.
const help = defineCollection({
  loader: glob({ pattern: "*.md", base: "./src/content/help" }),
  schema: z.object({
    title: z.string().min(1),
    summary: z.string().min(1),
    category: z.enum(ARTICLE_CATEGORIES),
    /** The day the steps were last checked against the product. */
    reviewed: z.coerce.date(),
  }),
});

// Legal pages: Markdown files in `src/content/legal`. The file name is the stable slug; `history`
// lists the newest change first, and its first line is the page's version and effective date.
const legal = defineCollection({
  loader: glob({ pattern: "*.md", base: "./src/content/legal" }),
  schema: z.object({
    title: z.string().min(1),
    summary: z.string().min(1),
    version: z.string().min(1),
    effective: z.coerce.date(),
    history: z
      .array(
        z.object({ version: z.string().min(1), date: z.coerce.date(), change: z.string().min(1) }),
      )
      .min(1),
  }),
});

export const collections = { help, legal };
