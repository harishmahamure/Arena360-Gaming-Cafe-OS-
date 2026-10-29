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
  player: "hub.audit.module.player",
  session: "hub.audit.module.session",
  alert: "hub.audit.module.alert",
  order: "hub.audit.module.order",
  library: "hub.audit.module.library",
};

/** Intent kinds with a catalogue label; any other kind shows as it is (`settings.set`). */
const INTENT_LABELS: Record<string, MessageKey> = {
  "staff.deactivate": "hub.audit.intent.staff.deactivate",
  "device.admin_exit": "hub.audit.intent.device.admin_exit",
  "device.admin_relock": "hub.audit.intent.device.admin_relock",
  "device.admin_unlock": "hub.audit.intent.device.admin_unlock",
  "device.screen": "hub.audit.intent.device.screen",
  "device.message": "hub.audit.intent.device.message",
  "device.power": "hub.audit.intent.device.power",
  "session.start": "hub.audit.intent.session.start",
  "session.end": "hub.audit.intent.session.end",
  "session.settle": "hub.audit.intent.session.settle",
  "wallet.topup": "hub.audit.intent.wallet.topup",
  "credit.set_limit": "hub.audit.intent.credit.set_limit",
  "wallet.topup.reverted": "hub.audit.intent.wallet.topup.reverted",
  "plan.sell": "hub.audit.intent.plan.sell",
  "plan.sell.reverted": "hub.audit.intent.plan.sell.reverted",
  "rule.upsert": "hub.audit.intent.rule.upsert",
  "coupon.upsert": "hub.audit.intent.coupon.upsert",
  "library.upsert": "hub.audit.intent.library.upsert",
  "library.delete": "hub.audit.intent.library.delete",
  "credit.collect": "hub.audit.intent.credit.collect",
  "credit.collect.reverted": "hub.audit.intent.credit.collect.reverted",
  "credit.auto_pause": "hub.audit.intent.credit.auto_pause",
  "session.pause": "hub.audit.intent.session.pause",
  "session.assign_player": "hub.audit.intent.session.assign_player",
  "session.resume": "hub.audit.intent.session.resume",
  "session.extend": "hub.audit.intent.session.extend",
  "session.expire": "hub.audit.intent.session.expire",
  "session.rate_flip": "hub.audit.intent.session.rate_flip",
  "invoice.issue": "hub.audit.intent.invoice.issue",
  "receipt.reprint": "hub.audit.intent.receipt.reprint",
  "receipt.printed": "hub.audit.intent.receipt.printed",
  "receipt.print_failed": "hub.audit.intent.receipt.print_failed",
  "drawer.kicked": "hub.audit.intent.drawer.kicked",
  "drawer.kick_failed": "hub.audit.intent.drawer.kick_failed",
  "session.transfer": "hub.audit.intent.session.transfer",
  "session.group": "hub.audit.intent.session.group",
  "group.payer": "hub.audit.intent.group.payer",
  "alert.raise": "hub.audit.intent.alert.raise",
  "alert.ack": "hub.audit.intent.alert.ack",
  "session.end.reverted": "hub.audit.intent.session.end.reverted",
  "session.force_end": "hub.audit.intent.session.force_end",
  "session.force_end.reverted": "hub.audit.intent.session.force_end.reverted",
  "controller.check": "hub.audit.intent.controller.check",
  "session.settle.reverted": "hub.audit.intent.session.settle.reverted",
  "player.update": "hub.audit.intent.player.update",
  "player.password_reset": "hub.audit.intent.player.password_reset",
  "player.password_change": "hub.audit.intent.player.password_change",
  "player.pc_settings_set": "hub.audit.intent.player.pc_settings_set",
  "ads.consent": "hub.audit.intent.ads.consent",
  "printer.status": "hub.audit.intent.printer.status",
  "order.place": "hub.audit.intent.order.place",
  "order.place.reverted": "hub.audit.intent.order.place.reverted",
  "order.line_served": "hub.audit.intent.order.line_served",
  "order.ready": "hub.audit.intent.order.ready",
  "kitchen.advance": "hub.audit.intent.kitchen.advance",
  "kitchen.advance.reverted": "hub.audit.intent.kitchen.advance.reverted",
  "kot.reprint": "hub.audit.intent.kot.reprint",
  "kot.printed": "hub.audit.intent.kot.printed",
  "kot.print_failed": "hub.audit.intent.kot.print_failed",
  "kds.pair": "hub.audit.intent.kds.pair",
  "kds.revoke": "hub.audit.intent.kds.revoke",
  "kds.seen": "hub.audit.intent.kds.seen",
  "order.line_cancel": "hub.audit.intent.order.line_cancel",
  "order.void": "hub.audit.intent.order.void",
  "refund.create": "hub.audit.intent.refund.create",
  "cash.deposit": "hub.audit.intent.cash.deposit",
  "expense.create": "hub.audit.intent.expense.create",
  "shift.close": "hub.audit.intent.shift.close",
  "shift.reprint_z": "hub.audit.intent.shift.reprint_z",
};

/** An intent kind's label. */
export function intentLabel(kind: string): string {
  const key = INTENT_LABELS[kind];
  return key ? t(key) : kind;
}

/** How a kiosk admin exit was granted. */
export const EXIT_MODE_LABELS: Record<string, MessageKey> = {
  hub: "hub.audit.exit.mode.hub",
  cached_verifier: "hub.audit.exit.mode.cached_verifier",
  remote: "hub.audit.exit.mode.remote",
  unknown: "hub.audit.exit.mode.unknown",
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
