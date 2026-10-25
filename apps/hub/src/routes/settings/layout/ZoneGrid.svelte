<script lang="ts">
  import { t } from "@arena/ui";
  import type { IntentPayload, SnapshotDevice, SnapshotZone } from "@arena/ui/generated";
  import { tick } from "svelte";

  const COLS = 12;
  const ROWS = 8;

  type Job = { payload: IntentPayload; rev?: number };
  let {
    zones,
    devices,
    run,
  }: { zones: SnapshotZone[]; devices: SnapshotDevice[]; run: (job: Job) => Promise<void> } =
    $props();

  let carrying: string | null = $state(null);
  let active: Record<string, { x: number; y: number }> = $state({});

  const carried = $derived(devices.find((d) => d.device_id === carrying));
  const zoneIds = $derived(new Set(zones.map((z) => z.id)));
  const unplaced = $derived(devices.filter((d) => !d.zone || !zoneIds.has(d.zone)));
  const grids: Record<string, HTMLElement | undefined> = {};
  const occupant = (z: string, x: number, y: number) =>
    devices.find((d) => d.zone === z && d.floor_x === x && d.floor_y === y);
  const free = (z: string, except: string) =>
    Array.from({ length: COLS * ROWS }, (_, i) => ({
      i,
      x: i % COLS,
      y: Math.floor(i / COLS),
    })).filter((c) => !occupant(z, c.x, c.y) || occupant(z, c.x, c.y)?.device_id === except);
  const at = (z: string) => active[z] ?? { x: 0, y: 0 };

  async function place(deviceId: string, zone: string | null, x: number | null, y: number | null) {
    const d = devices.find((v) => v.device_id === deviceId);
    if (!d) return;
    carrying = null;
    await run({
      payload: { kind: "device.place", device_id: deviceId, zone_id: zone, floor_x: x, floor_y: y },
      rev: d.meta_rev ?? undefined,
    });
    if (zone && x !== null && y !== null) await focusCell(zone, x, y);
  }

  async function focusCell(z: string, x: number, y: number) {
    active[z] = { x, y };
    await tick();
    (grids[z]?.children[y * COLS + x] as HTMLElement | undefined)?.focus();
  }

  async function pick(deviceId: string) {
    carrying = deviceId;
    const d = devices.find((v) => v.device_id === deviceId);
    if (d?.zone && zoneIds.has(d.zone))
      return void (await focusCell(d.zone, d.floor_x ?? 0, d.floor_y ?? 0));
    const first = zones[0];
    if (first) await focusCell(first.id, at(first.id).x, at(first.id).y);
  }

  async function click(z: string, x: number, y: number) {
    if (carrying) {
      if (occupant(z, x, y)?.device_id === carrying) carrying = null;
      else await place(carrying, z, x, y);
      return;
    }
    const d = occupant(z, x, y);
    if (d) await pick(d.device_id);
  }

  function key(e: KeyboardEvent, z: string, x: number, y: number) {
    const step: Record<string, [number, number]> = {
      ArrowLeft: [-1, 0],
      ArrowRight: [1, 0],
      ArrowUp: [0, -1],
      ArrowDown: [0, 1],
    };
    const s = step[e.key];
    if (!s) return;
    e.preventDefault();
    const nx = Math.min(COLS - 1, Math.max(0, x + s[0]));
    const ny = Math.min(ROWS - 1, Math.max(0, y + s[1]));
    void focusCell(z, nx, ny);
  }

  function drop(e: DragEvent, z: string, x: number, y: number) {
    e.preventDefault();
    void place(e.dataTransfer?.getData("text/plain") ?? "", z, x, y);
  }

  async function setZone(d: SnapshotDevice, value: string) {
    if (!value) return void (await place(d.device_id, null, null, null));
    const cell = free(value, d.device_id)[0];
    if (cell) await place(d.device_id, value, cell.x, cell.y);
  }

  async function setPosition(d: SnapshotDevice, value: string) {
    const i = Number(value);
    if (d.zone) await place(d.device_id, d.zone, i % COLS, Math.floor(i / COLS));
  }

  const current = (id: string) => devices.find((d) => d.device_id === id);
  const zoneValue = (id: string) => {
    const z = current(id)?.zone;
    return z && zoneIds.has(z) ? z : "";
  };
  const positionValue = (id: string) =>
    zoneValue(id) ? String((current(id)?.floor_y ?? 0) * COLS + (current(id)?.floor_x ?? 0)) : "";
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && (carrying = null)} />

<div class="wide">
  <p class="sr" role="status" data-testid="zone-carrying">
    {carried ? t("hub.zones.carrying", { device: carried.label }) : ""}
  </p>
  {#each zones as z (z.id)}
    <section class="zone" aria-labelledby="zone-title-{z.id}" data-testid="zone-section">
      <h3 class="type-hub-title" id="zone-title-{z.id}">{z.name}</h3>
      <div
        class="grid"
        bind:this={grids[z.id]}
        role="group"
        aria-label={t("hub.zones.grid", { zone: z.name })}
      >
        {#each Array.from({ length: ROWS }, (_, y) => y) as y (y)}
          {#each Array.from({ length: COLS }, (_, x) => x) as x (x)}
            {@const d = occupant(z.id, x, y)}
            <button
              type="button"
              class="cell ui-interactive type-hub-label"
              class:full={d}
              class:lifted={d && d.device_id === carrying}
              aria-pressed={d ? d.device_id === carrying : undefined}
              tabindex={at(z.id).x === x && at(z.id).y === y ? 0 : -1}
              aria-label={d
                ? t("hub.zones.cell_device", { device: d.label, zone: z.name, x: x + 1, y: y + 1 })
                : t("hub.zones.cell_empty", { zone: z.name, x: x + 1, y: y + 1 })}
              draggable={!!d}
              ondragstart={(e) => d && e.dataTransfer?.setData("text/plain", d.device_id)}
              ondragover={(e) => e.preventDefault()}
              ondrop={(e) => drop(e, z.id, x, y)}
              onclick={() => void click(z.id, x, y)}
              onkeydown={(e) => key(e, z.id, x, y)}
              data-testid={d ? "zone-tile" : "zone-cell"}
              data-x={x}
              data-y={y}
              data-device-id={d?.device_id}
            >
              {d?.label ?? ""}
            </button>
          {/each}
        {/each}
      </div>
    </section>
  {/each}

  <section class="pool" aria-labelledby="zone-pool-title">
    <h3 class="type-hub-title" id="zone-pool-title">{t("hub.zones.unplaced")}</h3>
    {#if unplaced.length === 0}
      <p class="type-hub-body">{t("hub.zones.unplaced_none")}</p>
    {:else}
      <ul class="rows" data-testid="zone-pool">
        {#each unplaced as d (d.device_id)}
          <li>
            <button
              type="button"
              class="btn ui-interactive type-hub-label"
              disabled={zones.length === 0}
              draggable="true"
              ondragstart={(e) => e.dataTransfer?.setData("text/plain", d.device_id)}
              onclick={() => void pick(d.device_id)}
              data-testid="zone-pool-device"
              data-device-id={d.device_id}>{t("hub.zones.pick", { device: d.label })}</button
            >
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<div class="narrow">
  <h3 class="type-hub-title">{t("hub.zones.devices")}</h3>
  <ul class="rows" data-testid="zone-list-devices">
    {#each devices as d (d.device_id)}
      {@const inZone = !!d.zone && zoneIds.has(d.zone)}
      <li class="row" data-testid="zone-list-device" data-device-id={d.device_id}>
        <span class="type-hub-title">{d.label}</span>
        <label class="field type-hub-label">
          <span>{t("hub.zones.zone")}</span>
          <select
            class="ui-interactive select"
            value={inZone ? d.zone : ""}
            onchange={async (e) => {
              const el = e.currentTarget;
              await setZone(d, el.value);
              await tick();
              el.value = zoneValue(d.device_id);
            }}
          >
            <option value="">{t("hub.zones.unassigned")}</option>
            {#each zones as z (z.id)}<option value={z.id}>{z.name}</option>{/each}
          </select>
        </label>
        <label class="field type-hub-label">
          <span>{t("hub.zones.position")}</span>
          <select
            class="ui-interactive select"
            disabled={!inZone}
            value={inZone ? (d.floor_y ?? 0) * COLS + (d.floor_x ?? 0) : ""}
            onchange={async (e) => {
              const el = e.currentTarget;
              await setPosition(d, el.value);
              await tick();
              el.value = positionValue(d.device_id);
            }}
          >
            {#if inZone && d.zone}
              {#each free(d.zone, d.device_id) as c (c.i)}
                <option value={c.i}
                  >{t("hub.zones.position_option", { x: c.x + 1, y: c.y + 1 })}</option
                >
              {/each}
            {/if}
          </select>
        </label>
      </li>
    {/each}
  </ul>
</div>

<style>
  .wide,
  .zone,
  .pool,
  .narrow {
    display: grid;
    gap: var(--space-3);
  }

  .wide {
    display: none;
  }

  @media (min-width: 840px) {
    .wide {
      display: grid;
    }

    .narrow {
      display: none;
    }
  }

  h3,
  p {
    margin: 0;
  }

  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(12, minmax(0, 1fr));
    gap: var(--space-1);
  }

  .cell {
    min-width: 0;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border: 1px dashed var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface);
    color: var(--text);
    cursor: pointer;
  }

  .cell.full {
    border-style: solid;
    background-color: var(--surface-2);
  }

  .cell.lifted {
    outline: 2px dashed var(--text);
  }

  .rows {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background-color: var(--surface);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  .select,
  .btn {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
  }

  .btn {
    cursor: pointer;
  }
</style>
