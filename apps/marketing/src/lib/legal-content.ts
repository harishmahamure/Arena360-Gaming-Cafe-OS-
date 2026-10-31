import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { getCollection } from "astro:content";
import { LEGAL_SLUGS, POLICY_VERSION_SOURCE, assertLegal, policyVersionFrom } from "./legal";

// Reads the Legal pages at build time. Pages import from here, so a Privacy policy version that
// differs from the `arena-proto` constant fails every build that touches Legal.

/** The policy-version constant kiosk sign-up records, read from the `arena-proto` source. */
export function policyVersion(): string | null {
  const path = resolve(process.cwd(), "..", "..", POLICY_VERSION_SOURCE);
  return policyVersionFrom(readFileSync(path, "utf8"));
}

/** Every Legal page in the order Legal lists them, after the version check. */
export async function legalDocs() {
  const entries = await getCollection("legal");
  const docs = entries.map((entry) => ({ slug: entry.id, ...entry.data }));
  assertLegal(docs, policyVersion());
  return LEGAL_SLUGS.map((slug) => docs.find((d) => d.slug === slug)!);
}
