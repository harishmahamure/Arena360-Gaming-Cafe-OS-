// The Legal pages' rules: which pages exist, and that the Privacy policy's version is the one kiosk
// sign-up records. Pure, with no imports, so the script tests can load it too. Words live in the
// message catalogue (`site.legal.*`); the policies are Markdown in `src/content/legal`.

/** The four Legal pages, by stable slug, in the order Legal lists them. */
export const LEGAL_SLUGS = ["terms", "privacy", "data-processing", "ad-policy"] as const;
export type LegalSlug = (typeof LEGAL_SLUGS)[number];

/** The Privacy policy: its version must be the constant kiosk sign-up consent rows record. */
export const PRIVACY_SLUG: LegalSlug = "privacy";

/** The `arena-proto` file that holds the policy-version constant. */
export const POLICY_VERSION_SOURCE = "crates/arena-proto/src/player_signup.rs";

/** One line of a page's change history. */
export interface LegalChange {
  version: string;
  date: Date;
  change: string;
}

/** What the checks need from a Legal page. */
export interface LegalDoc {
  slug: string;
  version: string;
  effective: Date;
  history: LegalChange[];
}

/** The value of `PLAYER_POLICY_VERSION` in the Rust source, or `null` when it is not there. */
export function policyVersionFrom(source: string): string | null {
  const match = /pub\s+const\s+PLAYER_POLICY_VERSION\s*:\s*&str\s*=\s*"([^"]+)"\s*;/.exec(source);
  return match?.[1] ?? null;
}

/**
 * Throws (so the build fails) when a Legal page is missing or extra, when a page's version is not
 * the newest line of its change history, or when the Privacy policy's version differs from the
 * policy-version constant.
 */
export function assertLegal(docs: LegalDoc[], constant: string | null): void {
  const slugs = docs.map((d) => d.slug).sort();
  const want = [...LEGAL_SLUGS].sort();
  if (slugs.join() !== want.join())
    throw new Error(`Legal pages must be ${want.join(", ")}; found ${slugs.join(", ")}`); // i18n-ignore
  for (const doc of docs) {
    const newest = doc.history[0];
    if (!newest || newest.version !== doc.version)
      throw new Error(
        `Legal page ${doc.slug}: version ${doc.version} is not the newest history line`, // i18n-ignore
      );
    if (newest.date.getTime() !== doc.effective.getTime())
      throw new Error(`Legal page ${doc.slug}: effective date is not the newest history date`); // i18n-ignore
  }
  if (constant === null)
    throw new Error(`${POLICY_VERSION_SOURCE} has no PLAYER_POLICY_VERSION constant`); // i18n-ignore
  const privacy = docs.find((d) => d.slug === PRIVACY_SLUG);
  if (privacy && privacy.version !== constant)
    throw new Error(
      `Privacy policy version ${privacy.version} differs from PLAYER_POLICY_VERSION ${constant}`, // i18n-ignore
    );
}
