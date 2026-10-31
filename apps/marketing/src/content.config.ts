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

export const collections = { help };
