// Manage → Products (Story 8.24): the products and bundles each branch holds, the Hub's field rules
// for editing them, and the per-branch commands an edit becomes. Like pricing, ids differ per
// branch: forms hold names (a bundle's components too) and `resolve()` matches them per branch.

import { t } from "@arena/ui";
import type { CatalogueRow, SummaryCatalogue } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import type { CommandDraft, Excluded, Variant } from "./commands.svelte";
import {
  badAmount,
  choices,
  fieldsOf,
  idByName,
  missing,
  money,
  moneyText,
  norm,
  num,
  parsePrice,
  percentInput,
  priceText,
  resolve,
  sourceOf,
  str,
  type BranchData,
  type Choice,
  type Ctx,
  type Entry,
  type Listed,
} from "./pricing";
import { ulid } from "./ulid";

type Errors = Record<string, MessageKey>;

const MAX_NAME = 80;
const MAX_CATEGORY = 40;
const MAX_BARCODE = 48;
const MAX_PARTS = 30;
const MAX_PART_QTY = 1000;
const MAX_THRESHOLD = 100_000_000;

// --- What each branch holds ---

export const productsOf = (c: SummaryCatalogue) => c.products;
export const bundlesOf = (c: SummaryCatalogue) => c.bundles;
const everything = (c: SummaryCatalogue) => [...c.products, ...c.bundles];

/** The products a bundle can hold (a bundle cannot hold a bundle), by name over the branches. */
export const partChoices = (data: BranchData[]): Choice[] => choices(data, productsOf);

export const isBundle = (row: CatalogueRow) => fieldsOf(row).bundle === true;
export const barcodeOf = (row: CatalogueRow) => str(fieldsOf(row).barcode) ?? "";

/** A product is tracked when it holds stock: only then it has a low-stock level. */
export const tracked = (row: CatalogueRow) =>
  !!row.fields && fieldsOf(row).track_stock !== false && !isBundle(row);

/** The item matches what the owner typed: part of its name or of a barcode at any branch. */
export function matches(item: Listed, query: string): boolean {
  const q = norm(query);
  return (
    q === "" ||
    item.entries.some((e) => norm(e.row.name).includes(q) || norm(barcodeOf(e.row)).includes(q))
  );
}

/** The price and tax of a row in words, e.g. `₹50.00 · 12%`; empty for a row without its fields. */
export function priceLine(e: Entry): string {
  const f = fieldsOf(e.row);
  const price = num(f.price_minor);
  if (price === undefined) return "";
  const rate = num(f.tax_rate_bp);
  const amount = moneyText(price, e.data.currency);
  return rate === undefined
    ? amount
    : t("hub.products.price_tax", { price: amount, rate: percentInput(rate) });
}

/** The components of a bundle row in words, e.g. `Contains 2 × Cola`. */
export function containsText(e: Entry): string {
  const list = fieldsOf(e.row).components;
  if (!Array.isArray(list) || !list.length) return "";
  const items = (list as { product_id: string; qty: number }[]).flatMap((c) => {
    const p = e.data.cat?.products.find((x) => x.id === c.product_id);
    return p ? [t("hub.products.component_item", { qty: c.qty, name: p.name })] : [];
  });
  return items.length ? t("hub.products.component_list", { items: items.join(", ") }) : "";
}

/** The low-stock level of a tracked row, if it has one. */
export const thresholdOf = (row: CatalogueRow) => num(fieldsOf(row).low_stock_qty);

// --- The form ---

export type PartLine = { name: string; qty: string };
export type ProductForm = {
  bundle: boolean;
  name: string;
  category: string;
  price: string;
  /** Percent; empty uses the branch's default rate. */
  tax: string;
  hsn: string;
  barcode: string;
  track: boolean;
  kitchen: boolean;
  favourite: boolean;
  active: boolean;
  parts: PartLine[];
  /** `undefined` keeps the picture each branch holds, `""` removes it, a data URL replaces it. */
  image: string | undefined;
};

export const blankProduct = (bundle = false): ProductForm => ({
  bundle,
  name: "",
  category: "",
  price: "",
  tax: "",
  hsn: "",
  barcode: "",
  track: !bundle,
  kitchen: false,
  favourite: false,
  active: true,
  parts: bundle ? [{ name: "", qty: "1" }] : [],
  image: undefined,
});

export function productFormOf(item: Listed): ProductForm {
  const e = sourceOf(item);
  if (!e) return blankProduct();
  const f = fieldsOf(e.row);
  const tax = num(f.tax_rate_bp);
  const list = Array.isArray(f.components)
    ? (f.components as { product_id: string; qty: number }[])
    : [];
  return {
    bundle: isBundle(e.row),
    name: e.row.name,
    category: str(f.category) ?? "",
    price: priceText(num(f.price_minor) ?? 0, e.data.exponent),
    tax: tax === undefined ? "" : percentInput(tax),
    hsn: str(f.hsn) ?? "",
    barcode: str(f.barcode) ?? "",
    track: f.track_stock !== false,
    kitchen: f.kitchen === true,
    favourite: f.favourite === true,
    active: e.row.active ?? true,
    parts: list.map((c) => ({
      name: e.data.cat?.products.find((p) => p.id === c.product_id)?.name ?? "",
      qty: String(c.qty),
    })),
    image: undefined,
  };
}

/** A tax percentage such as `5` or `12.5` as basis points up to 100%, or `null`. */
export function parseTax(text: string): number | null {
  const m = /^(\d{1,3})(?:\.(\d{1,2}))?$/.exec(text.trim());
  if (!m) return null;
  const bp = Number(m[1]) * 100 + Number((m[2] ?? "").padEnd(2, "0") || 0);
  return bp <= 10_000 ? bp : null;
}

export type ProductValues = {
  bundle: boolean;
  name: string;
  category: string;
  price: string;
  tax?: number;
  hsn: string;
  barcode: string;
  track: boolean;
  kitchen: boolean;
  favourite: boolean;
  active: boolean;
  parts: { name: string; qty: number }[];
  image?: string;
};

export function validateProduct(f: ProductForm, exponent: number) {
  const errors: Errors = {};
  const name = f.name.trim();
  if (name.length < 1 || [...name].length > MAX_NAME || /\p{Cc}/u.test(name)) {
    errors.name = "hub.products.invalid.name";
  }
  const category = f.category.trim();
  if ([...category].length > MAX_CATEGORY || /\p{Cc}/u.test(category)) {
    errors.category = "hub.products.invalid.category";
  }
  if (parsePrice(f.price, exponent) === null) errors.price = "hub.products.invalid.price";
  const tax = f.tax.trim() === "" ? undefined : parseTax(f.tax);
  if (tax === null) errors.tax = "hub.products.invalid.tax";
  const hsn = f.hsn.trim();
  if (hsn !== "" && !/^(\d{4}|\d{6}|\d{8})$/.test(hsn)) errors.hsn = "hub.products.invalid.hsn";
  const barcode = f.barcode.trim();
  if ([...barcode].length > MAX_BARCODE || !/^[\x21-\x7e]*$/.test(barcode)) {
    errors.barcode = "hub.products.invalid.barcode";
  }
  const parts = f.parts.map((p) => ({
    name: p.name,
    qty: /^\d{1,4}$/.test(p.qty.trim()) ? Number(p.qty.trim()) : 0,
  }));
  if (f.bundle) {
    const names = parts.map((p) => norm(p.name));
    if (
      parts.length < 1 ||
      parts.length > MAX_PARTS ||
      parts.some((p) => p.name.trim() === "" || p.qty < 1 || p.qty > MAX_PART_QTY) ||
      new Set(names).size !== names.length ||
      names.includes(norm(name))
    ) {
      errors.parts = "hub.products.invalid.components";
    }
  }
  const values: ProductValues = {
    bundle: f.bundle,
    name,
    category,
    price: f.price,
    hsn,
    barcode,
    track: f.track && !f.bundle,
    kitchen: f.kitchen,
    favourite: f.favourite,
    active: f.active,
    parts: f.bundle ? parts : [],
    ...(typeof tax === "number" ? { tax } : {}),
    ...(f.image === undefined ? {} : { image: f.image }),
  };
  return { errors, values, ok: Object.keys(errors).length === 0 };
}

// --- From a form to the commands ---

/**
 * The `product.upsert` of a product or bundle for each branch in scope. The picture is sent only
 * when the owner chose one (or removed it): the Hub keeps the stored picture of a command without.
 */
export function proposeProduct(ctx: Ctx, v: ProductValues): CommandDraft {
  return resolve(ctx, {
    kind: "product.upsert",
    label: v.name,
    list: everything,
    name: norm(v.name),
    make: (d, cat) => {
      const components: { product_id: string; qty: number }[] = [];
      for (const p of v.parts) {
        const product_id = idByName(cat.products, p.name);
        if (product_id === undefined) return missing(p.name);
        components.push({ product_id, qty: p.qty });
      }
      const price = money(v.price, d);
      if (price === null) return badAmount();
      const self = ctx.origin ?? norm(v.name);
      const taken = everything(cat).find(
        (r) =>
          r.active !== false && norm(r.name) !== self && v.barcode && barcodeOf(r) === v.barcode,
      );
      if (v.active && taken) {
        return { reason: t("owner.products.excluded.barcode", { name: taken.name }) };
      }
      return {
        payload: {
          name: v.name,
          category: v.category,
          price_minor: price,
          ...(v.tax === undefined ? {} : { tax_rate_bp: v.tax }),
          hsn: v.hsn,
          track_stock: v.track,
          barcode: v.barcode,
          kitchen: v.kitchen,
          favourite: v.favourite,
          active: v.active,
          bundle: v.bundle,
          components,
          ...(v.image === undefined ? {} : { image: v.image }),
        },
      };
    },
  });
}

// --- The low-stock level ---

/** A whole number from 0 to the Hub's limit, or empty to clear the level. */
export function validateThreshold(text: string): { value: number | null } | { error: MessageKey } {
  if (text.trim() === "") return { value: null };
  if (!/^\d{1,9}$/.test(text.trim())) return { error: "hub.lowstock.invalid" };
  const n = Number(text.trim());
  return n <= MAX_THRESHOLD ? { value: n } : { error: "hub.lowstock.invalid" };
}

/**
 * `stock.threshold` for the tracked product of that name at each branch in scope. There is no
 * "create" here: a branch without the product, or with it untracked, is left out with the reason.
 */
export function proposeThreshold(
  ctx: Pick<Ctx, "scope" | "data">,
  name: string,
  value: number | null,
): CommandDraft {
  const variants: Variant[] = [];
  const excluded: Excluded[] = [];
  for (const d of ctx.data) {
    const branch_id = d.branch.id;
    if (!d.cat) {
      excluded.push({ branch_id, reason: t("owner.pricing.excluded.nodata") });
      continue;
    }
    const row = d.cat.products.find((r) => norm(r.name) === norm(name));
    if (!row || !tracked(row)) {
      excluded.push({ branch_id, reason: t("owner.products.excluded.untracked", { name }) });
      continue;
    }
    variants.push({
      branch_id,
      payload: { product_id: row.id, low_stock_qty: value },
      key: ulid(),
    });
  }
  return {
    kind: "stock.threshold",
    payload: { name, low_stock_qty: value },
    branch_id: ctx.scope,
    key: ulid(),
    variants,
    excluded,
    choices: {},
  };
}
