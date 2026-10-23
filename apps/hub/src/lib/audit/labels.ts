// How the audit viewer names actors, kinds, modules and operations, all through the catalogue.

import { t } from "@arena/ui";
import type { AuditActor, AuditActorKind, ChangeOpKind } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";

export const KIND_LABELS: Record<AuditActorKind, MessageKey> = {
  staff: "hub.audit.kind.staff",
  device: "hub.audit.kind.device",
  system: "hub.audit.kind.system",
  owner: "hub.audit.kind.owner",
  admin: "hub.audit.kind.admin",
  unknown: "hub.audit.kind.unknown",
};

/** The fixed actor names the Hub sends as catalogue keys. */
const FIXED_NAMES: Record<string, MessageKey> = {
  "hub.audit.actor.system": "hub.audit.actor.system",
  "hub.audit.actor.owner": "hub.audit.actor.owner",
  "hub.audit.actor.admin": "hub.audit.actor.admin",
};

const MODULE_LABELS: Record<string, MessageKey> = {
  branch: "hub.audit.module.branch",
  settings: "hub.audit.module.settings",
  device: "hub.audit.module.device",
  console: "hub.audit.module.console",
  staff: "hub.audit.module.staff",
  role: "hub.audit.module.role",
  audit: "hub.audit.module.audit",
  kiosk: "hub.audit.module.kiosk",
};

const OP_LABELS: Record<ChangeOpKind, MessageKey> = {
  insert: "hub.audit.op.insert",
  update: "hub.audit.op.update",
  delete: "hub.audit.op.delete",
};

/** An actor's shown name: a staff name or device label as sent, a fixed key translated. */
export function actorName(actor: AuditActor): string {
  const key = FIXED_NAMES[actor.name];
  const fixed = actor.kind === "system" || actor.kind === "owner" || actor.kind === "admin";
  return key && fixed ? t(key) : actor.name;
}

/** The actor's name with its kind, e.g. "Asha · Staff". */
export function actorLabel(actor: AuditActor): string {
  return `${actorName(actor)} · ${t(KIND_LABELS[actor.kind])}`; // i18n-ignore (a joiner)
}

/** A module's label; a domain the catalogue does not know yet shows as it is. */
export function moduleLabel(module: string): string {
  const key = MODULE_LABELS[module];
  return key ? t(key) : module;
}

export function opLabel(op: ChangeOpKind): MessageKey {
  return OP_LABELS[op];
}
