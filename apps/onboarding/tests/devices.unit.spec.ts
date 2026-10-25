import { expect, test } from "@playwright/test";
import type { SetupDevicesView } from "@arena/ui/generated";
import { age, fits, overlay } from "../src/lib/devices";

const view = (over: Partial<SetupDevicesView>): SetupDevicesView => ({
  setup: {
    devices: [{ id: "D1", label: "PC-1", kind: "pc", group_id: null, status: "online" }],
    groups: [{ id: "G1", name: "Gaming", kind: "pc", colour: "teal", sort: 0 }],
  },
  reported_at: 1000,
  now: 1000 + 130_000,
  hub_online: true,
  commands: [],
  ...over,
});
const command = (kind: string, payload: unknown, status = "pending") =>
  ({
    key: `K-${kind}`,
    kind,
    payload,
    status,
    error: null,
  }) as SetupDevicesView["commands"][number];

test("a simulator fits an Other group and a PC does not", () => {
  expect(fits("sim", { kind: "other" })).toBe(true);
  expect(fits("pc", { kind: "console" })).toBe(false);
});

test("waiting commands overlay the reported state in order", () => {
  const shown = overlay(
    view({
      commands: [
        command("group.upsert", { name: "VR", group_kind: "vr", colour: "lime", sort_order: 1 }),
        command("device.assign", { device_id: "D1", group_id: "G1" }),
        command("device.register", { class: "paired", device_kind: "console", label: "PS5" }),
      ],
    }),
  );
  expect(shown.groups.map((g) => [g.name, g.waiting])).toEqual([
    ["Gaming", false],
    ["VR", true],
  ]);
  expect(shown.devices.find((d) => d.id === "D1")).toMatchObject({ group_id: "G1", waiting: true });
  expect(shown.devices.at(-1)).toMatchObject({ label: "PS5", kind: "console", waiting: true });
});

test("a refused assignment leaves the device in place and is reported for it", () => {
  const refused = {
    ...command("device.assign", { device_id: "D1", group_id: "G1" }, "rejected"),
    error: { code: "GROUP_KIND_MISMATCH", message: "no" },
  } as SetupDevicesView["commands"][number];
  const shown = overlay(view({ commands: [refused] }));
  expect(shown.devices[0]).toMatchObject({ group_id: null, waiting: false });
  expect(shown.refusals).toMatchObject([{ subject: "D1", kind: "device.assign" }]);
});

test("the age is whole minutes since the Hub last reported", () => {
  expect(age(view({}))).toBe(2);
  expect(age(view({ reported_at: null }))).toBeNull();
});
