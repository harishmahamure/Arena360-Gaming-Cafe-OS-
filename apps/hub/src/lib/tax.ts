export const KEY_TAX = "branch.tax";

export type Tax = {
  country: string;
  gst: boolean;
  gstin: string;
  inclusive: boolean;
  product_bp: number;
  play_bp: number;
  sac: string;
  label: string;
  rate_bp: number;
};

type Form = {
  country: string;
  gst: boolean;
  gstin: string;
  inclusive: string;
  product: string;
  play: string;
  sac: string;
  label: string;
  rate: string;
};

const isBp = (v: unknown) => typeof v === "number" && Number.isInteger(v) && v >= 0 && v <= 10000;

export function parseTax(value: string): Tax | null {
  try {
    const o = JSON.parse(value) as Tax;
    const ok =
      typeof o.country === "string" &&
      typeof o.gst === "boolean" &&
      typeof o.gstin === "string" &&
      typeof o.inclusive === "boolean" &&
      isBp(o.product_bp) &&
      isBp(o.play_bp) &&
      isBp(o.rate_bp) &&
      typeof o.sac === "string" &&
      typeof o.label === "string";
    return ok ? o : null;
  } catch {
    return null;
  }
}

export const pct = (bp: number) => String(bp / 100);

export function parsePct(text: string): number | null {
  const m = /^(\d{1,3})(?:\.(\d{1,2}))?$/.exec(text.trim());
  const bp = m ? Number(m[1]) * 100 + Number((m[2] ?? "").padEnd(2, "0")) : null;
  return bp !== null && bp <= 10000 ? bp : null;
}

export function taxJson(base: Tax, f: Form): string {
  const india = f.country.trim().toUpperCase() === "IN";
  const next: Tax = {
    ...base,
    country: f.country.trim().toUpperCase(),
    inclusive: f.inclusive === "true",
  };
  if (india) {
    next.gst = f.gst;
    next.gstin = f.gst ? f.gstin.trim().toUpperCase() : "";
    next.product_bp = parsePct(f.product) ?? 10001;
    next.play_bp = parsePct(f.play) ?? 10001;
    next.sac = f.sac.trim();
  } else {
    next.gst = false;
    next.gstin = "";
    next.label = f.label.trim();
    next.rate_bp = parsePct(f.rate) ?? 10001;
  }
  return JSON.stringify(next);
}

const NAMES: Record<string, string> = { product_bp: "product", play_bp: "play", rate_bp: "rate" };

export function fieldName(field: string | undefined): string {
  const f = (field ?? "").replace("branch.tax.", "");
  return NAMES[f] ?? f;
}
