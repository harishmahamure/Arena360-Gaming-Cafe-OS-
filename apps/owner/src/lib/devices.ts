// Manage → Devices and groups (Story 8.27): the groups and devices each branch holds, the Hub's
// field rules for a group, and the commands an edit becomes. Groups are told apart by name over
// the branches in scope (like Pricing). A device belongs to one branch, so moving or revoking it
// is one command to that branch, with that branch's own ids.

import { t } from "@arena/ui";
import type { CatalogueDevice, CatalogueRow } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";
import type { CommandDraft } from "./commands.svelte";
import {
  fieldsOf,
  norm,
  num,
  resolve,
  str,
  type BranchData,
  type Ctx,
  type Listed,
} from "./pricing";
import { ulid } from "./ulid";

type Errors = Record<string, MessageKey>;

const MAX_NAME = 80;

/** The kinds a group can have and the six swatches, as the Hub accepts them. */
export const GROUP_KINDS = [
  { id: "pc", label: "owner.devices.kind.pc" },
  { id: "console", label: "owner.devices.kind.console" },
  { id: "vr", label: "owner.devices.kind.vr" },
  { id: "other", label: "owner.devices.kind.other" },
] as const satisfies readonly { id: string; label: MessageKey }[];

export const GROUP_COLOURS = [
  { id: "teal", label: "owner.devices.colour.teal" },
  { id: "orange", label: "owner.devices.colour.orange" },
  { id: "lime", label: "owner.devices.colour.lime" },
  { id: "sand", label: "owner.devices.colour.sand" },
  { id: "coral", label: "owner.devices.colour.coral" },
  { id: "silver", label: "owner.devices.colour.silver" },
] as const satisfies readonly { id: string; label: MessageKey }[];

/** The reasons the Hub lists for revoking a device. */
export const REVOKE_REASONS = [
  { id: "lost", label: "owner.devices.reason.lost" },
  { id: "stolen", label: "owner.devices.reason.stolen" },
  { id: "replaced", label: "owner.devices.reason.replaced" },
  { id: "other", label: "owner.devices.reason.other" },
] as const satisfies readonly { id: string; label: MessageKey }[];

// --- What each branch holds ---

/** A device with the branch that holds it and the name of its group. */
export type DeviceLine = {
  data: BranchData;
  device: CatalogueDevice;
  group: CatalogueRow | undefined;
};

/** The devices of every branch in scope, by branch and name. */
export function deviceLines(data: BranchData[]): DeviceLine[] {
  return data.flatMap((d) =>
    (d.cat?.devices ?? []).map((device) => ({
      data: d,
      device,
      group: d.cat?.groups.find((g) => g.id === device.group_id),
    })),
  );
}

/** The kind of group a device of this kind can join (the Hub's rule). */
export const groupKindOf = (deviceKind: string) => (deviceKind === "sim" ? "other" : deviceKind);

/** The groups of the device's branch it may be moved to. */
export const groupsFor = (line: DeviceLine): CatalogueRow[] =>
  (line.data.cat?.groups ?? []).filter(
    (g) =>
      str(fieldsOf(g).group_kind) === undefined ||
      str(fieldsOf(g).group_kind) === groupKindOf(line.device.kind),
  );

/** A group's colour, `sand` when an older summary didn't send it. */
export const colourOf = (row: CatalogueRow) => str(fieldsOf(row).colour) ?? "sand";

/** A group can be edited when its branch sent the fields. */
export const groupEditable = (row: CatalogueRow) => !!row.fields;

// --- The group form ---

export type GroupForm = { name: string; kind: string; colour: string };

export const blankGroup = (): GroupForm => ({ name: "", kind: "pc", colour: "teal" });

export function groupFormOf(item: Listed): GroupForm {
  const e = item.entries.find((x) => x.row.fields);
  if (!e) return blankGroup();
  const f = fieldsOf(e.row);
  return {
    name: e.row.name,
    kind: str(f.group_kind) ?? "pc",
    colour: str(f.colour) ?? "teal",
  };
}

export type GroupValues = GroupForm;

export function validateGroup(f: GroupForm) {
  const errors: Errors = {};
  const name = f.name.trim();
  if (name.length < 1 || [...name].length > MAX_NAME || /\p{Cc}/u.test(name)) {
    errors.name = "owner.devices.invalid.name";
  }
  if (!GROUP_KINDS.some((k) => k.id === f.kind)) errors.kind = "owner.devices.invalid.kind";
  if (!GROUP_COLOURS.some((c) => c.id === f.colour)) errors.colour = "owner.devices.invalid.colour";
  const values: GroupValues = { name, kind: f.kind, colour: f.colour };
  return { errors, values, ok: Object.keys(errors).length === 0 };
}

// --- From a form to the commands ---

/** `group.upsert` for the group of that name at each branch in scope. */
export function proposeGroup(ctx: Ctx, v: GroupValues): CommandDraft {
  return resolve(ctx, {
    kind: "group.upsert",
    label: v.name,
    list: (c) => c.groups,
    name: norm(v.name),
    make: (_d, cat) => {
      const there = cat.groups.find((g) => norm(g.name) === (ctx.origin ?? norm(v.name)));
      const order = there ? num(fieldsOf(there).sort_order) : undefined;
      const next = Math.max(0, ...cat.groups.map((g) => num(fieldsOf(g).sort_order) ?? 0)) + 1;
      return {
        payload: {
          name: v.name,
          group_kind: v.kind,
          colour: v.colour,
          sort_order: order ?? next,
        },
      };
    },
  });
}

/** `device.assign`: moves one device to a group of its branch, or to Unassigned (`null`). */
export function proposeAssign(line: DeviceLine, groupId: string | null): CommandDraft {
  const branch_id = line.data.branch.id;
  return {
    kind: "device.assign",
    payload: { name: line.device.name },
    branch_id,
    key: ulid(),
    variants: [
      {
        branch_id,
        payload: { device_id: line.device.id, group_id: groupId },
        key: ulid(),
      },
    ],
    excluded: [],
    choices: {},
  };
}

/** `device.revoke`: a sensitive command, so sending it passes step-up MFA. */
export function proposeRevoke(line: DeviceLine, reason: string): CommandDraft {
  const branch_id = line.data.branch.id;
  return {
    kind: "device.revoke",
    payload: { name: line.device.name },
    branch_id,
    key: ulid(),
    variants: [
      {
        branch_id,
        payload: { device_id: line.device.id, reason },
        key: ulid(),
      },
    ],
    excluded: [],
    choices: {},
  };
}

/** A device's kind in the owner's words. */
export const deviceKindText = (kind: string): string =>
  t(
    (
      {
        pc: "owner.devices.kind.pc",
        console: "owner.devices.kind.console",
        vr: "owner.devices.kind.vr",
        sim: "owner.devices.kind.sim",
      } as Record<string, MessageKey>
    )[kind] ?? "owner.devices.kind.other",
  );
