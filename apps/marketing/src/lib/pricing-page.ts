// The Pricing page's numbers and table, all read from the committed snapshots of the live price book
// and of the `arena-proto` tier table, never typed into page copy.

import type { MessageKey } from "@arena/ui/messages";
import {
  GST_PERCENT,
  addOnPrice,
  monthsFree,
  planPrice,
  priceFor,
  tierRow,
  type AddOnKey,
  type BillingPeriod,
  type PlanKey,
  type TierRow,
} from "@arena/ui/pricing";
import { rupees } from "./price";

export const PLAN_KEYS: readonly PlanKey[] = ["free", "basic", "premium"];
export const PAID_KEYS = ["basic", "premium"] as const;
export const BILLING: readonly BillingPeriod[] = ["monthly", "yearly"];

export type Cell =
  | { kind: "yes" }
  | { kind: "no" }
  | { kind: "number"; n: number }
  | { kind: "text"; key: MessageKey; params?: Record<string, string | number> };

export type TableRow = { label: MessageKey; cells: Record<PlanKey, Cell> };

export type TierCard = {
  key: PlanKey;
  /** The price of each billing cycle, formatted: `₹200`, `₹2,000`. */
  price: Record<BillingPeriod, string>;
  /** Months of a yearly price that are free; 0 on Free. */
  monthsFree: number;
  points: { key: MessageKey; params?: Record<string, number> }[];
};

const YES: Cell = { kind: "yes" };
const NO: Cell = { kind: "no" };
const UNLIMITED: Cell = { kind: "text", key: "site.pricing.cell.unlimited" };

const flag = (row: TierRow, name: string): Cell => (row.flags.includes(name) ? YES : NO);
const cap = (n: number | null): Cell => (n === null ? UNLIMITED : { kind: "number", n });

function backup(row: TierRow): Cell {
  if (row.upload_interval_s === 0) return { kind: "text", key: "site.pricing.cell.manual_export" };
  const years = Math.round(row.retention_days / 365);
  return {
    kind: "text",
    key: years === 1 ? "site.pricing.cell.backup" : "site.pricing.cell.backup_years",
    params: { minutes: Math.round(row.upload_interval_s / 60), years },
  };
}

/** The comparison table: every tier's limits and flags as the `arena-proto` tier table has them. */
export function comparisonRows(): TableRow[] {
  const rows = Object.fromEntries(PLAN_KEYS.map((key) => [key, tierRow(key)])) as Record<
    PlanKey,
    TierRow
  >;
  const row = (label: MessageKey, cell: (r: TierRow, key: PlanKey) => Cell): TableRow => ({
    label,
    cells: Object.fromEntries(PLAN_KEYS.map((key) => [key, cell(rows[key], key)])) as Record<
      PlanKey,
      Cell
    >,
  });
  return [
    row("site.pricing.table.branches", (r) => cap(r.branches)),
    row("site.pricing.table.devices", (r) => cap(r.devices)),
    row("site.pricing.table.users", (r) => cap(r.users)),
    row("site.pricing.table.owner", (r) => flag(r, "owner_app")),
    row("site.pricing.table.backup", (r) => backup(r)),
    row("site.pricing.table.standby", (r) => flag(r, "standby")),
    // Premium includes the Tunnel; Basic can buy it as an add-on.
    row("site.pricing.table.tunnel", (r, key) =>
      r.flags.includes("tunnel")
        ? YES
        : key === "basic"
          ? { kind: "text", key: "site.pricing.cell.addon" }
          : NO,
    ),
    row("site.pricing.table.kitchen", (r) => flag(r, "kitchen")),
    // Ads run only on the Free Android Hub; every paid tier is ad-free.
    row("site.pricing.table.ads", (_r, key) => ({
      kind: "text",
      key: key === "free" ? "site.pricing.cell.ads_android" : "site.pricing.cell.ads_none",
    })),
  ];
}

export function tierCards(): TierCard[] {
  return PLAN_KEYS.map((key) => {
    const plan = planPrice(key);
    const row = tierRow(key);
    return {
      key,
      price: {
        monthly: rupees(priceFor(plan, "monthly")),
        yearly: rupees(priceFor(plan, "yearly")),
      },
      monthsFree: monthsFree(plan),
      points: [
        { key: "site.pricing.point.devices", params: { n: row.devices } },
        row.users === null
          ? { key: "site.pricing.point.users_unlimited" }
          : { key: "site.pricing.point.users", params: { n: row.users } },
      ],
    };
  });
}

/** Add-on rows in display order, with their monthly price. */
export const ADDONS: readonly { item: AddOnKey; label: MessageKey }[] = [
  { item: "addon_devices_20", label: "site.pricing.addons.devices" },
  { item: "addon_tunnel", label: "site.pricing.addons.tunnel" },
  { item: "addon_owner_login", label: "site.pricing.addons.owner_login" },
];

export const addOnMonthly = (item: AddOnKey) => rupees(addOnPrice(item).monthly);

export const gstPercent = () => String(GST_PERCENT);

/** The text a screen reader hears when the billing cycle changes. */
export function announcements(): Record<BillingPeriod, MessageKey> {
  return {
    monthly: "site.pricing.announce.monthly",
    yearly: "site.pricing.announce.yearly",
  };
}

/** Values for the announcement and the billing note: both paid plans in `period`, and months free. */
export function paidParams(period: BillingPeriod) {
  return {
    basic: rupees(priceFor(planPrice("basic"), period)),
    premium: rupees(priceFor(planPrice("premium"), period)),
    gst: gstPercent(),
    months: String(monthsFree(planPrice("basic"))),
  };
}
