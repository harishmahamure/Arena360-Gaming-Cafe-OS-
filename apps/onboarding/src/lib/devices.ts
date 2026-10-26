import type {
  AppError,
  SetupCommandView,
  SetupDevice,
  SetupDevicesView,
  SetupGroup,
} from "@arena/ui/generated";

export type Waiting<T> = T & { waiting: boolean };
export type Refusal = { key: string; kind: string; subject: string | null; error: AppError };

export const fits = (deviceKind: string, group: { kind: string }) =>
  (deviceKind === "sim" ? "other" : deviceKind) === group.kind;

const field = (c: SetupCommandView, name: string) =>
  (c.payload as Record<string, unknown>)[name] as string | undefined;

export function overlay(view: SetupDevicesView) {
  const groups: Waiting<SetupGroup>[] = (view.setup?.groups ?? []).map((g) => ({
    ...g,
    waiting: false,
  }));
  const devices: Waiting<SetupDevice>[] = (view.setup?.devices ?? []).map((d) => ({
    ...d,
    waiting: false,
  }));
  const refusals: Refusal[] = [];
  for (const c of view.commands) {
    if (c.kind === "setup.test_session") continue;
    if (c.status === "rejected") {
      const subject = c.kind === "device.assign" ? (field(c, "device_id") ?? null) : null;
      if (c.error) refusals.push({ key: c.key, kind: c.kind, subject, error: c.error });
      continue;
    }
    const p = c.payload as Record<string, never>;
    if (c.kind === "group.upsert") {
      const next = {
        id: field(c, "id") ?? c.key,
        name: p.name,
        kind: p.group_kind,
        colour: p.colour,
        sort: Number(p.sort_order ?? 0),
        has_rate: false,
        waiting: true,
      } as Waiting<SetupGroup>;
      const at = groups.findIndex((g) => g.id === next.id);
      if (at >= 0) groups[at] = next;
      else groups.push(next);
    } else if (c.kind === "device.assign") {
      const d = devices.find((x) => x.id === field(c, "device_id"));
      if (d) Object.assign(d, { group_id: field(c, "group_id") ?? null, waiting: true });
    } else if (c.kind === "device.register") {
      devices.push({
        id: c.key,
        label: p.label,
        kind: p.device_kind,
        group_id: field(c, "group_id") ?? null,
        status: "offline",
        waiting: true,
      });
    }
  }
  groups.sort((a, b) => a.sort - b.sort);
  return { groups, devices, refusals };
}

export function age(view: SetupDevicesView): number | null {
  return view.reported_at === null
    ? null
    : Math.max(0, Math.floor((view.now - view.reported_at) / 60000));
}
