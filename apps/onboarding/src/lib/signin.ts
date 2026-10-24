const CROCKFORD = /^[0-9A-HJKMNP-TV-Z]{10}$/;

export function normalizeBackupCode(input: string): string | null {
  const s = input.replace(/[-\s]/g, "").toUpperCase().replace(/[IL]/g, "1").replace(/O/g, "0");
  return CROCKFORD.test(s) ? s : null;
}
