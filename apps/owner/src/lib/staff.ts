// Manage → Staff users and roles (Story 8.26): the team each branch holds, the roles with their
// permissions, the Hub's field rules for a member and a role, and the per-branch commands an edit
// becomes. Ids are per branch: a person is told apart by their login (a mobile number) and a role
// by its name, and `proposeStaff*` builds one variant per branch with that branch's own ids.

import { t } from "@arena/ui";
import type { CatalogueRow, CatalogueStaff, Permission } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import type { CommandDraft, Excluded, Variant } from "./commands.svelte";
import {
  fieldsOf,
  norm,
  resolve,
  type BranchData,
  type Ctx,
  type Listed,
  choices,
  type Choice,
} from "./pricing";
import { ulid } from "./ulid";

type Errors = Record<string, MessageKey>;

const MAX_NAME = 80;
/** The Hub's built-in roles: a custom role may not take their names. */
const BUILT_IN = ["owner", "manager", "counter", "kitchen", "kiosk-device"];
/** The role of devices only: never offered for a person. */
const DEVICE_ROLE = "kiosk-device";

// --- The permission catalogue ---

// Keyed by every permission, so a permission added to the catalogue stops the type check here.
const CATALOGUE: Record<Permission, true> = {
  "player.manage": true,
  "player.view": true,
  "wallet.topup": true,
  "credit.manage": true,
  "credit.collect": true,
  "session.start": true,
  "session.pause": true,
  "session.extend": true,
  "session.transfer": true,
  "session.end": true,
  "session.force_end": true,
  "booking.manage": true,
  "pos.sell": true,
  "pos.void": true,
  "refund.issue": true,
  "discount.manual": true,
  "kitchen.use": true,
  "stock.in": true,
  "stock.adjust": true,
  "stock.override": true,
  "catalog.edit": true,
  "shift.open": true,
  "shift.close": true,
  "shift.override_variance": true,
  "cash.manage": true,
  "device.manage": true,
  "device.remote": true,
  "device.pair": true,
  "device.screen": true,
  "user.manage": true,
  "settings.edit": true,
  "games.manage": true,
  "backup.export": true,
  "ops.promote": true,
  "ops.restore": true,
  "report.view": true,
  "audit.view": true,
};

/** Every permission a role can hold, in the catalogue's order. */
export const PERMISSIONS = Object.keys(CATALOGUE) as Permission[];

// --- The team ---

/** One branch's row of a person. */
export type Member = { data: BranchData; staff: CatalogueStaff };

/** A person of the café: the staff rows of every branch that share a login. */
export type Person = { key: string; name: string; login: string; members: Member[] };

/** The staff of every branch grouped by login (a row without one stands alone). */
export function people(data: BranchData[]): Person[] {
  const map = new Map<string, Person>();
  for (const d of data) {
    for (const staff of d.cat?.staff ?? []) {
      const key = staff.login ? norm(staff.login) : `id:${staff.id}`; // i18n-ignore
      const person = map.get(key) ?? {
        key,
        name: staff.name,
        login: staff.login ?? "",
        members: [],
      };
      person.members.push({ data: d, staff });
      map.set(key, person);
    }
  }
  return [...map.values()];
}

/** The person matches what the owner typed: part of their name or their number. */
export function matches(p: Person, query: string): boolean {
  const q = norm(query);
  return q === "" || norm(p.name).includes(q) || norm(p.login).includes(q.replace(/[\s()-]/g, ""));
}

/** A person can be deactivated where they are active. */
export const canDeactivate = (p: Person) => p.members.some((m) => m.staff.active);

/** The roles a person can hold, by name over the branches in scope. */
export const roleChoices = (data: BranchData[]): Choice[] =>
  choices(data, (c) => c.roles.filter((r) => norm(r.name) !== DEVICE_ROLE));

export const isBuiltIn = (row: CatalogueRow) => row.detail === "builtin";

/** The permissions a role row allows (an older summary lists none). */
export function allowedOf(row: CatalogueRow): Permission[] {
  const list = fieldsOf(row).permissions;
  if (!Array.isArray(list)) return [];
  return (list as { permission: string; effect: string }[])
    .filter((p) => p.effect === "allow" && Object.hasOwn(CATALOGUE, p.permission))
    .map((p) => p.permission as Permission);
}

/** A role row can be edited when it is custom and its branch sent the permissions. */
export const roleEditable = (row: CatalogueRow) => !isBuiltIn(row) && !!row.fields;

// --- The staff form ---

export type StaffForm = {
  name: string;
  phone: string;
  role: string;
  active: boolean;
  /** Issue a new setup code with this edit. */
  newCode: boolean;
};

export const blankStaff = (): StaffForm => ({
  name: "",
  phone: "",
  role: "",
  active: true,
  newCode: false,
});

/** The form of a person as the first branch that has them says; the role is the branch's own. */
export function staffFormOf(p: Person): StaffForm {
  const first = p.members[0].staff;
  return {
    name: first.name,
    phone: p.login,
    role: first.role,
    active: first.active,
    newCode: false,
  };
}

/** A mobile number with the country code, as the cloud checks it: `+`, 8–15 digits. */
export function parsePhone(text: string): string | null {
  const phone = text.replace(/[\s()-]/g, "");
  const digits = phone.slice(1);
  if (!phone.startsWith("+") || !/^[1-9]\d{7,14}$/.test(digits)) return null;
  const national = digits.startsWith("91") ? digits.slice(2) : null;
  return national === null || /^[6-9]\d{9}$/.test(national) ? phone : null;
}

export type StaffValues = {
  name: string;
  phone: string;
  role: string;
  active: boolean;
  newCode: boolean;
};

const plainName = (s: string) => s.length >= 1 && [...s].length <= MAX_NAME && !/\p{Cc}/u.test(s);

export function validateStaff(f: StaffForm, adding: boolean) {
  const errors: Errors = {};
  const name = f.name.trim();
  if (!plainName(name)) errors.name = "owner.staff.invalid.name";
  const phone = adding ? parsePhone(f.phone) : f.phone;
  if (adding && phone === null) errors.phone = "owner.staff.invalid.phone";
  if (f.role.trim() === "") errors.role = "owner.staff.invalid.role";
  const values: StaffValues = {
    name,
    phone: phone ?? "",
    role: f.role,
    active: f.active,
    newCode: f.newCode,
  };
  return { errors, values, ok: Object.keys(errors).length === 0 };
}

// --- From a form to the commands ---

const draft = (
  kind: string,
  payload: Record<string, unknown>,
  scope: string,
  variants: Variant[],
  excluded: Excluded[],
): CommandDraft => ({
  kind,
  payload,
  branch_id: scope,
  key: ulid(),
  variants,
  excluded,
  choices: {},
});

const noData = (branch_id: string): Excluded => ({
  branch_id,
  reason: t("owner.pricing.excluded.nodata"),
});

/**
 * `staff.upsert` that adds a member at each branch in scope. A branch without the role, or that
 * already has someone with that number, is left out with the reason. The cloud makes the setup
 * code: the app sends no credential of any kind.
 */
export function proposeStaffAdd(ctx: Pick<Ctx, "scope" | "data">, v: StaffValues): CommandDraft {
  const variants: Variant[] = [];
  const excluded: Excluded[] = [];
  for (const d of ctx.data) {
    const branch_id = d.branch.id;
    if (!d.cat) {
      excluded.push(noData(branch_id));
      continue;
    }
    const role = d.cat.roles.find((r) => norm(r.name) === norm(v.role));
    const there = d.cat.staff.find((s) => s.login && norm(s.login) === norm(v.phone));
    if (!role) {
      excluded.push({ branch_id, reason: t("owner.staff.excluded.role", { name: v.role }) });
    } else if (there) {
      excluded.push({ branch_id, reason: t("owner.staff.excluded.exists", { name: there.name }) });
    } else {
      variants.push({
        branch_id,
        payload: { login: v.phone, display_name: v.name, role_id: role.id, active: true },
        key: ulid(),
      });
    }
  }
  return draft("staff.upsert", { display_name: v.name }, ctx.scope, variants, excluded);
}

/** `staff.upsert` that edits a person where they are on the team, with each row's `base_rev`. */
export function proposeStaffEdit(
  ctx: Pick<Ctx, "scope" | "data">,
  person: Person,
  v: StaffValues,
): CommandDraft {
  const variants: Variant[] = [];
  const excluded: Excluded[] = [];
  for (const d of ctx.data) {
    const branch_id = d.branch.id;
    const member = person.members.find((m) => m.data.branch.id === branch_id)?.staff;
    if (!d.cat) {
      excluded.push(noData(branch_id));
      continue;
    }
    const role = d.cat.roles.find((r) => norm(r.name) === norm(v.role));
    if (!member) {
      excluded.push({ branch_id, reason: t("owner.staff.excluded.absent", { name: person.name }) });
    } else if (!role) {
      excluded.push({ branch_id, reason: t("owner.staff.excluded.role", { name: v.role }) });
    } else {
      variants.push({
        branch_id,
        payload: {
          staff_id: member.id,
          login: member.login ?? person.login,
          display_name: v.name,
          role_id: role.id,
          active: v.active,
          ...(v.newCode ? { setup_code: true } : {}),
        },
        base_rev: member.rev,
        key: ulid(),
      });
    }
  }
  return draft("staff.upsert", { display_name: v.name }, ctx.scope, variants, excluded);
}

/** `staff.deactivate` for a person at each branch in scope where they are still active. */
export function proposeDeactivate(ctx: Pick<Ctx, "scope" | "data">, person: Person): CommandDraft {
  const variants: Variant[] = [];
  const excluded: Excluded[] = [];
  for (const d of ctx.data) {
    const branch_id = d.branch.id;
    const member = person.members.find((m) => m.data.branch.id === branch_id)?.staff;
    if (!d.cat) {
      excluded.push(noData(branch_id));
    } else if (!member) {
      excluded.push({ branch_id, reason: t("owner.staff.excluded.absent", { name: person.name }) });
    } else if (!member.active) {
      excluded.push({
        branch_id,
        reason: t("owner.staff.excluded.inactive", { name: person.name }),
      });
    } else variants.push({ branch_id, payload: { staff_id: member.id }, key: ulid() });
  }
  return draft("staff.deactivate", { display_name: person.name }, ctx.scope, variants, excluded);
}

// --- The role form ---

export type RoleForm = { name: string; allowed: Permission[] };

export const blankRole = (): RoleForm => ({ name: "", allowed: [] });

export function roleFormOf(item: Listed): RoleForm {
  const e = item.entries.find((x) => x.row.fields);
  return e ? { name: e.row.name, allowed: allowedOf(e.row) } : blankRole();
}

export type RoleValues = { name: string; allowed: Permission[] };

export function validateRole(f: RoleForm) {
  const errors: Errors = {};
  const name = f.name.trim();
  if (!plainName(name)) errors.name = "owner.roles.invalid.name";
  else if (BUILT_IN.includes(norm(name))) errors.name = "owner.roles.invalid.builtin";
  const values: RoleValues = { name, allowed: PERMISSIONS.filter((p) => f.allowed.includes(p)) };
  return { errors, values, ok: Object.keys(errors).length === 0 };
}

/** `role.upsert` for the custom role of that name at each branch in scope. */
export function proposeRole(ctx: Ctx, v: RoleValues): CommandDraft {
  const made = resolve(ctx, {
    kind: "role.upsert",
    label: v.name,
    list: (c) => c.roles,
    name: norm(v.name),
    make: () => ({
      payload: {
        name: v.name,
        perms: v.allowed.map((permission) => ({ permission, effect: "allow" })),
      },
    }),
  });
  // A role's id travels as `role_id`.
  const named = ({ id, ...rest }: Record<string, unknown>) =>
    id === undefined ? rest : { role_id: id, ...rest };
  return { ...made, variants: made.variants?.map((x) => ({ ...x, payload: named(x.payload) })) };
}
