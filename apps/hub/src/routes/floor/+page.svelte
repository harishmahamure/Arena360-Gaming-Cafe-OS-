<script lang="ts">
  import { AlertBadge, FloorTile, STATUS_LABELS, formatDateTime, formatMoney, t } from "@arena/ui";
  import type {
    DeviceStatus,
    SessionStaff,
    SnapshotDevice,
    SnapshotGroup,
    SnapshotSession,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { page } from "$app/state";
  import { tick, untrack } from "svelte";
  import { ackDevice, ackable, openFor, sheetRequest, tagOf } from "../../lib/alerts.svelte";
  import { nextTile } from "../../lib/floor-nav";
  import { liveRate } from "../../lib/rules";
  import TileSheet from "../../lib/TileSheet.svelte";
  import { PAUSE_LABELS, cmds, moved } from "../../lib/tile-actions.svelte";
  import UndoDock from "../../lib/UndoDock.svelte";
  import { viewport } from "../../lib/viewport.svelte";
  import type { PageData } from "./$types";

  let { data }: { data: PageData } = $props();

  const staff = $derived.by(() => {
    const session = (page.data as { session?: { state: string; staff?: SessionStaff } }).session;
    return session?.state === "signed_in" ? (session.staff ?? null) : null;
  });

  type Badge = {
    state: "free" | "offline" | "in-session" | "paused" | "reserved" | "maintenance";
    label: MessageKey;
    /** Draws the status badge as a warning while the tile keeps its own state. */
    badgeState?: "warning";
  };
  const BADGE: Record<DeviceStatus, Badge> = {
    online: { state: "free", label: "hub.floor.online" },
    offline: { state: "offline", label: "hub.floor.offline" },
    in_session: { state: "in-session", label: "ui.status.in_session" },
    reserved: { state: "reserved", label: "ui.status.reserved" },
    maintenance: { state: "maintenance", label: "ui.status.maintenance" },
  };

  const snapshot = $derived(data.hub.ok ? data.hub.snapshot : null);
  const sessionOf = (d: SnapshotDevice) =>
    snapshot?.sessions?.find((s) => s.device_id === d.device_id);
  const names = $derived(
    Object.fromEntries((snapshot?.devices ?? []).map((d) => [d.device_id, d.label])),
  );
  const unpaid = (d: SnapshotDevice) =>
    (snapshot?.bills ?? [])
      .filter((b) => b.device_id === d.device_id && b.order_id)
      .reduce((sum, b) => sum + b.remaining_minor, 0);
  let now = $state(Date.now());
  $effect(() => {
    if (!snapshot?.sessions?.length) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const two = (n: number) => String(n).padStart(2, "0");
  const WARN_MS = 10 * 60_000;
  const timed = (session: SnapshotSession) => session.state === "active" && session.ends_at != null;
  const inGrace = (session: SnapshotSession) =>
    session.state === "grace" && session.grace_ends_at != null;
  const nearEnd = (session: SnapshotSession) =>
    (timed(session) && session.ends_at! - now <= WARN_MS) ||
    inGrace(session) ||
    session.state === "expired";
  const leftText = (session: SnapshotSession) => {
    if (!timed(session)) return undefined;
    const left = Math.ceil((session.ends_at! - now) / 60_000);
    return left > 10
      ? undefined
      : t("hub.floor.min_left", { n: left <= 1 ? 1 : left <= 5 ? 5 : 10 });
  };
  const isConsole = (session: SnapshotSession) =>
    snapshot?.devices.find((d) => d.device_id === session.device_id)?.kind === "console";
  const clock = (s: number) =>
    [Math.floor(s / 3600), two(Math.floor(s / 60) % 60), two(s % 60)].join(":");
  const elapsed = (session: SnapshotSession) => {
    if (inGrace(session)) {
      const s = Math.max(0, Math.ceil((session.grace_ends_at! - now) / 1000));
      return t("hub.floor.grace", { time: [Math.floor(s / 60), two(s % 60)].join(":") });
    }
    if (session.state === "expired")
      return t(isConsole(session) ? "hub.floor.times_up" : "hub.floor.time_up");
    if (timed(session)) return clock(Math.max(0, Math.ceil((session.ends_at! - now) / 1000)));
    const end = session.paused_at ?? now;
    const s = Math.max(
      0,
      Math.floor((end - session.started_at - (session.paused_total_ms ?? 0)) / 1000),
    );
    return [Math.floor(s / 3600), two(Math.floor(s / 60) % 60), two(s % 60)].join(":");
  };
  const badgeOf = (d: SnapshotDevice, session: SnapshotSession | undefined): Badge =>
    session?.state === "paused"
      ? { state: "paused", label: "ui.status.paused" }
      : d.controller_check && d.status === "online"
        ? { state: "free", label: "hub.floor.check_controllers", badgeState: "warning" }
        : (BADGE[d.status ?? "offline"] ?? BADGE.offline);
  const pauseText = (session: SnapshotSession) =>
    session.pause_reason === "other"
      ? (session.pause_note ?? "")
      : session.pause_reason
        ? t(PAUSE_LABELS[session.pause_reason])
        : "";
  const tz = $derived(snapshot?.locale?.tz);
  const groups = $derived(snapshot?.groups ?? []);
  const zones = $derived(snapshot?.zones ?? []);
  const placed = (d: SnapshotDevice) => zones.some((z) => z.id === d.zone);
  const inZone = (id: string) =>
    (snapshot?.devices ?? [])
      .filter((d) => d.zone === id)
      .sort((a, b) => (a.floor_y ?? 0) - (b.floor_y ?? 0) || (a.floor_x ?? 0) - (b.floor_x ?? 0));
  const unassigned = $derived(
    snapshot?.devices.filter((d) => !placed(d) && !groups.some((g) => g.id === d.group_id)) ?? [],
  );
  const loose = (id: string) =>
    snapshot?.devices.filter((d) => d.group_id === id && !placed(d)) ?? [];
  const COLLAPSED_KEY = "arena.hub.floor.collapsed";
  const stored = (): string[] => {
    try {
      const ids = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "[]");
      return Array.isArray(ids) ? ids : [];
    } catch {
      return [];
    }
  };
  let collapsed: string[] = $state(stored());
  function toggle(id: string) {
    collapsed = collapsed.includes(id) ? collapsed.filter((c) => c !== id) : [...collapsed, id];
    try {
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify(collapsed));
    } catch {
      return;
    }
  }
  const STATES = [
    "in-session",
    "paused",
    "free",
    "reserved",
    "maintenance",
    "offline",
    "unassigned",
  ] as const;
  const stateOf = (d: SnapshotDevice) =>
    groups.some((g) => g.id === d.group_id) ? badgeOf(d, sessionOf(d)).state : "unassigned";
  const counts = (members: SnapshotDevice[]) =>
    STATES.map((s) => [s, members.filter((d) => stateOf(d) === s).length] as const)
      .filter(([, n]) => n > 0)
      .map(([s, n]) =>
        t("hub.floor.state_count", { count: n, state: t(STATUS_LABELS[s]).toLowerCase() }),
      )
      .join(" · ");
  const order = $derived([
    ...zones.filter((z) => !collapsed.includes(z.id)).flatMap((z) => inZone(z.id)),
    ...groups.flatMap((g) => loose(g.id)),
    ...unassigned,
  ]);
  let focusId: string | null = $state(null);
  let activeId: string | null = $state(page.url.searchParams.get("device"));
  const active = $derived(snapshot?.devices.find((d) => d.device_id === activeId));
  $effect(() => {
    if (activeId && !active) activeId = null;
  });
  let sheet: TileSheet | undefined = $state();
  let transferring = $state(false);
  const activeSession = $derived(active ? sessionOf(active) : undefined);
  $effect(() => {
    if (transferring && !(activeSession && ["active", "paused"].includes(activeSession.state))) {
      transferring = false;
    }
  });
  const isTarget = (d: SnapshotDevice) =>
    transferring &&
    active !== undefined &&
    d.device_id !== active.device_id &&
    d.kind === active.kind &&
    d.status === "online" &&
    groups.some((g) => g.id === d.group_id);
  let grouping = $state(false);
  /** Picking a tile on the floor (a transfer or grouping) needs the floor in reach. */
  const picking = $derived(transferring || grouping);
  /** On a phone the sheet is full height over the floor, which is then inert. */
  const full = $derived(viewport.phone && active !== undefined && !picking);
  $effect(() => {
    viewport.sheetFull = full;
    return () => (viewport.sheetFull = false);
  });
  $effect(() => {
    const s = activeSession;
    if (grouping && !(s && s.mode === "postpaid" && ["active", "paused"].includes(s.state))) {
      grouping = false;
    }
  });
  const groupKind = (d: SnapshotDevice): "add" | "start" | null => {
    if (!grouping || !active || d.device_id === active.device_id) return null;
    const s = sessionOf(d);
    if (s) {
      const open = s.mode === "postpaid" && ["active", "paused"].includes(s.state);
      return open && s.group_bill_id == null ? "add" : null;
    }
    return d.kind === "pc" && d.status === "online" && groups.some((g) => g.id === d.group_id)
      ? "start"
      : null;
  };
  function pickForGroup(d: SnapshotDevice) {
    const s = sessionOf(d);
    if (s) sheet?.groupWith(s.id);
    else if (groupKind(d) === "start") sheet?.startInGroup(d);
  }
  async function startGrouping() {
    grouping = true;
    await tick();
    document.querySelector<HTMLElement>("[data-group-target] [data-tile-face]")?.focus();
  }
  async function stopGrouping() {
    grouping = false;
    await tick();
    document.querySelector<HTMLElement>(`[data-device-id="${activeId}"] [data-tile-face]`)?.focus();
  }
  const payerOf = (id: string | undefined) => {
    const g = snapshot?.group_bills?.find((x) => x.id === id);
    if (!g) return "";
    return g.payer_username ?? t("hub.session.guest", { n: g.payer_guest_no ?? 0 });
  };
  const groupViewOf = (id: string | undefined) => {
    const g = snapshot?.group_bills?.find((x) => x.id === id);
    if (!g) return undefined;
    const members = (snapshot?.sessions ?? []).filter((s) => s.group_bill_id === g.id);
    const bills = snapshot?.bills ?? [];
    const lines = [
      ...g.ended_lines,
      ...bills
        .filter((b) => !b.order_id && members.some((m) => m.id === b.session_id))
        .flatMap((b) => b.lines),
    ];
    return { group: g, members, lines };
  };
  const unpaidBill = (d: SnapshotDevice) =>
    snapshot?.bills?.find((b) => b.device_id === d.device_id && b.order_id);
  let seenMove = moved.n;
  $effect(() => {
    const n = moved.n;
    if (n === seenMove) return;
    seenMove = n;
    if (moved.deviceId) {
      activeId = moved.deviceId;
      transferring = false;
    }
  });
  async function startTransfer() {
    transferring = true;
    await tick();
    document.querySelector<HTMLElement>("[data-transfer-target] [data-tile-face]")?.focus();
  }
  async function cancelTransfer() {
    transferring = false;
    await tick();
    document.querySelector<HTMLElement>(`[data-device-id="${activeId}"] [data-tile-face]`)?.focus();
  }
  let seenRequest = sheetRequest.n;
  $effect(() => {
    const n = sheetRequest.n;
    if (n === seenRequest) return;
    seenRequest = n;
    activeId = sheetRequest.deviceId;
  });
  const activeDeviceId = $derived(active?.device_id);
  $effect(() => {
    const id = activeDeviceId;
    if (id && ackable(id).length > 0) untrack(() => void ackDevice(id));
  });
  const zoneAlerts = (members: SnapshotDevice[]) =>
    members.reduce((sum, d) => sum + openFor(d.device_id).length, 0);
  async function closeSheet() {
    const id = activeId;
    activeId = null;
    await tick();
    document.querySelector<HTMLElement>(`[data-device-id="${id}"] [data-tile-face]`)?.focus();
  }
  const tabStop = $derived(
    order.find((d) => d.device_id === focusId)?.device_id ?? order[0]?.device_id,
  );
  const count = (n: number) =>
    t(n === 1 ? "hub.floor.count_one" : "hub.floor.count_other", { count: n });

  const face = (e: Event) => (e.target as HTMLElement).closest<HTMLElement>("[data-tile-face]");

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && transferring) {
      e.preventDefault();
      void cancelTransfer();
      return;
    }
    if (e.key === "Escape" && grouping) {
      e.preventDefault();
      void stopGrouping();
      return;
    }
    const current = e.key.startsWith("Arrow") && face(e);
    if (!current || e.target !== current) return;
    const faces = [...document.querySelectorAll<HTMLElement>("[data-tile-face]")];
    e.preventDefault();
    faces[nextTile(faces, faces.indexOf(current), e.key)]?.focus();
  }

  function onfocusin(e: FocusEvent) {
    const id = face(e)?.closest<HTMLElement>("[data-device-id]")?.dataset.deviceId;
    if (id) focusId = id;
  }
</script>

<svelte:window {onkeydown} {onfocusin} />

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.nav.floor") })}</title>
</svelte:head>

{#snippet tile(device: SnapshotDevice, group: SnapshotGroup | undefined, cell = false)}
  {@const session = group ? sessionOf(device) : undefined}
  {@const badge = badgeOf(device, session)}
  {@const owed = unpaid(device)}
  {@const live = liveRate(group)}
  {@const rate = live != null && snapshot?.locale ? formatMoney(live, snapshot.locale) : null}
  <FloorTile
    tag="li"
    name={device.label}
    state={group ? badge.state : "unassigned"}
    label={group ? badge.label : undefined}
    badgeState={group ? badge.badgeState : undefined}
    time={session ? elapsed(session) : undefined}
    warning={session ? nearEnd(session) : false}
    warningText={session && device.kind === "console" ? leftText(session) : undefined}
    console={device.kind === "console"}
    alertTag={tagOf(device.device_id)}
    tabindex={tabStop === device.device_id ? 0 : -1}
    {group}
    note={isTarget(device)
      ? t("hub.transfer.move_here")
      : groupKind(device)
        ? t(groupKind(device) === "add" ? "hub.group.add_here" : "hub.group.start_here")
        : undefined}
    onactivate={() =>
      transferring
        ? sheet?.moveTo(device.device_id)
        : grouping
          ? pickForGroup(device)
          : (activeId = device.device_id)}
    data-transfer-target={isTarget(device) ? "" : undefined}
    data-group-target={groupKind(device) ? "" : undefined}
    style={cell ? `--x:${(device.floor_x ?? 0) + 1};--y:${(device.floor_y ?? 0) + 1}` : undefined}
    aria-current={activeId === device.device_id ? "true" : undefined}
    data-device-id={device.device_id}
    data-testid={cell ? "zone-device" : group ? "group-device" : "unassigned-device"}
  >
    {#snippet secondary()}
      {#if !group}<span data-testid="device-presence">{t(badge.label)}</span>{/if}
      {#if isTarget(device)}
        <span data-testid="transfer-move-here">{t("hub.transfer.move_here")}</span>
      {:else if groupKind(device)}
        <span data-testid="group-pick-here"
          >{t(groupKind(device) === "add" ? "hub.group.add_here" : "hub.group.start_here")}</span
        >
      {:else if cmds[device.device_id]?.state === "failed"}
        <span data-testid="tile-failed"
          ><span aria-hidden="true">⚠</span> {t("hub.floor.remote_unlock_failed")}</span
        >
      {:else if device.status === "maintenance"}
        <span data-testid="device-unavailable"
          >{t("hub.floor.unavailable")}{device.maintenance_note
            ? ` · ${device.maintenance_note}`
            : ""}</span
        >
      {:else if device.status === "offline" && device.last_seen_at}
        <span data-testid="device-last-seen"
          >{t("hub.floor.last_seen", { at: formatDateTime(device.last_seen_at, tz) })}</span
        >
      {:else if session?.state === "paused"}
        <span data-testid="pause-line">{pauseText(session)}</span>
      {:else if session}
        <span data-testid="session-line"
          >{session.username ?? t("hub.session.guest", { n: session.guest_no ?? 0 })}</span
        >
        {#if session.group_bill_id && payerOf(session.group_bill_id)}
          <span data-testid="group-marker"
            >{t("hub.group.marker", { payer: payerOf(session.group_bill_id) })}</span
          >
        {/if}
      {:else if device.status === "online" && group && owed > 0 && snapshot?.locale}
        <span data-testid="tile-unpaid"
          >{t("hub.floor.unpaid", { amount: formatMoney(owed, snapshot.locale) })}</span
        >
      {:else if device.status === "online" && group}
        <span>{rate ? t("hub.floor.free_rate", { rate }) : t("hub.floor.tap_to_start")}</span>
      {/if}
    {/snippet}
    {#if device.tv_number}<span class="type-hub-label" data-testid="tv-number"
        >{t("hub.floor.tv", { n: device.tv_number })}</span
      >{/if}
  </FloorTile>
{/snippet}

<UndoDock {staff} />

<div class="stage" class:open={active !== undefined} class:picking>
  <div class="floor" inert={full}>
    <h1 class="type-hub-numeric">{t("hub.nav.floor")}</h1>

    {#if snapshot && !snapshot.branch}
      <section class="note" data-testid="floor-not-set-up">
        <p class="type-hub-title">{t("hub.floor.not_set_up")}</p>
        <p class="type-hub-body muted">{t("hub.floor.not_set_up_body")}</p>
      </section>
    {:else if snapshot}
      {#if zones.length === 0}
        <p class="type-hub-body muted" data-testid="floor-no-zones">{t("hub.floor.no_zones")}</p>
      {/if}

      {#each zones as zone (zone.id)}
        {@const members = inZone(zone.id)}
        {@const folded = collapsed.includes(zone.id)}
        <section
          class="unassigned zone"
          aria-labelledby="floor-zone-{zone.id}"
          data-testid="floor-zone"
        >
          <div class="head">
            <h2 class="type-hub-title" id="floor-zone-{zone.id}">
              <button
                type="button"
                class="fold"
                aria-expanded={!folded}
                title={t("hub.floor.collapse_hint")}
                onclick={() => toggle(zone.id)}
              >
                {zone.name}
              </button>
            </h2>
            <span class="muted type-hub-label">{count(members.length)}</span>
            {#if zoneAlerts(members) > 0}
              <span class="type-hub-label zone-alerts" data-testid="zone-alerts"
                >{t("hub.alerts.button")}
                <AlertBadge count={zoneAlerts(members)} live={false} /></span
              >
            {/if}
            {#if folded}
              <span class="type-hub-label" data-testid="zone-counts">{counts(members)}</span>
            {/if}
          </div>
          {#if !folded}
            <ul
              class="devices cells"
              style="--cols:{Math.max(
                6,
                ...members.map((d) => (d.floor_x ?? 0) + 1),
              )};--rows:{Math.max(1, ...members.map((d) => (d.floor_y ?? 0) + 1))}"
            >
              {#each members as device (device.device_id)}
                {@render tile(
                  device,
                  groups.find((g) => g.id === device.group_id),
                  true,
                )}
              {/each}
            </ul>
          {/if}
        </section>
      {/each}

      {#each groups as group (group.id)}
        {@const members = loose(group.id)}
        {#if members.length > 0}
          <section class="unassigned" aria-labelledby="floor-group-{group.id}">
            <div class="head">
              <h2 class="type-hub-title" id="floor-group-{group.id}">{group.name}</h2>
              <span class="muted type-hub-label">{count(members.length)}</span>
            </div>
            <ul class="devices">
              {#each members as device (device.device_id)}
                {@render tile(device, group)}
              {/each}
            </ul>
          </section>
        {/if}
      {/each}

      <section class="unassigned" aria-labelledby="floor-unassigned-title">
        <div class="head">
          <h2 class="type-hub-title" id="floor-unassigned-title">{t("hub.floor.unassigned")}</h2>
          <span class="muted type-hub-label">{count(unassigned.length)}</span>
        </div>
        {#if unassigned.length === 0}
          <p class="type-hub-body muted">{t("hub.floor.no_devices")}</p>
        {:else}
          <ul class="devices">
            {#each unassigned as device (device.device_id)}
              {@render tile(device, undefined)}
            {/each}
          </ul>
        {/if}
      </section>
    {/if}
  </div>
  {#if active}
    {@const group = groups.find((g) => g.id === active.group_id)}
    {#key active.device_id}
      <TileSheet
        bind:this={sheet}
        {transferring}
        ontransfer={startTransfer}
        oncanceltransfer={cancelTransfer}
        {grouping}
        ongroup={startGrouping}
        oncancelgroup={stopGrouping}
        groupView={groupViewOf(
          sessionOf(active)?.group_bill_id ?? unpaidBill(active)?.group_bill_id,
        )}
        device={active}
        {group}
        {groups}
        zone={zones.find((z) => z.id === active.zone)?.name}
        rate={liveRate(group) != null && snapshot?.locale
          ? formatMoney(liveRate(group) ?? 0, snapshot.locale)
          : null}
        badge={badgeOf(active, sessionOf(active))}
        {pauseText}
        {staff}
        {tz}
        session={sessionOf(active)}
        bill={snapshot?.bills?.find((b) => b.device_id === active.device_id && !b.order_id)}
        unpaid={snapshot?.bills?.find((b) => b.device_id === active.device_id && b.order_id)}
        locale={snapshot?.locale ?? null}
        {names}
        onclose={closeSheet}
      />
    {/key}
  {/if}
</div>

<style>
  .stage {
    display: grid;
    gap: var(--space-5);
    align-items: start;
  }

  .stage.open {
    grid-template-columns: minmax(0, 1fr);
  }

  .stage > :global([data-component="tile-sheet"]) {
    position: sticky;
    top: var(--space-4);
    /* --ad-shrink is set only with the ad-banner slot, which the sheet must stop above. */
    max-height: calc(100dvh - 2 * var(--space-4) - var(--ad-shrink, 0px));
    overflow-y: auto;
  }

  @media (min-width: 840px) {
    .stage.open {
      grid-template-columns: minmax(0, 1fr) 24rem;
    }
  }

  @media (max-width: 839.98px) {
    /* A transfer or grouping pick keeps the floor in reach: the sheet is a card at the bottom. */
    .stage.picking > :global([data-component="tile-sheet"]) {
      position: fixed;
      /* Above the ad-banner slot, with its clear space (--ad-reserved and --ad-gap: ads only). */
      inset: auto var(--space-3) calc(var(--ad-reserved) + var(--ad-gap, var(--space-3)));
      z-index: 5;
      max-height: calc(60dvh - var(--ad-reserved));
    }

    /* Otherwise it covers the floor, full height; its settle region is pinned inside it. */
    .stage.open:not(.picking) > :global([data-component="tile-sheet"]) {
      position: fixed;
      /* Ends at the top edge of the ad-banner slot (--ad-reserved is 0 without ads). */
      inset: 0 0 var(--ad-reserved);
      z-index: 5;
      max-height: none;
      border: 0;
      border-radius: 0;
    }
  }

  .floor {
    display: grid;
    gap: var(--space-5);
  }

  h1,
  h2,
  p {
    margin: 0;
  }

  .muted,
  .note {
    color: var(--text-muted);
  }

  .note {
    display: grid;
    gap: var(--space-2);
    max-width: 60ch;
  }

  .unassigned {
    display: grid;
    gap: var(--space-4);
  }

  .head {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
  }

  .zone-alerts {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }

  .fold {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 44px;
    padding: 0 var(--space-2);
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .fold::before {
    content: "▾" / "";
  }

  .fold[aria-expanded="false"]::before {
    content: "▸" / "";
  }

  .devices {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 220px), 1fr));
    gap: var(--space-4);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  @media (min-width: 1600px) {
    .devices:not(.cells) {
      grid-template-columns: repeat(6, 1fr);
    }

    .stage.open .devices:not(.cells) {
      grid-template-columns: repeat(4, 1fr);
    }
  }

  @media (max-width: 839px) {
    .devices {
      grid-template-columns: repeat(2, 1fr);
      gap: 12px;
    }
  }

  .zone {
    overflow-x: auto;
  }

  @media (min-width: 840px) {
    .cells {
      grid-template-columns: repeat(var(--cols), minmax(10rem, 1fr));
      grid-template-rows: repeat(var(--rows), minmax(4rem, auto));
    }

    .cells > :global([data-component="floor-tile"]) {
      grid-column: var(--x);
      grid-row: var(--y);
    }
  }

  @media (min-width: 1600px) {
    .stage.open .cells {
      grid-template-columns: repeat(4, minmax(0, 1fr));
      grid-template-rows: none;
    }

    .stage.open .cells > :global([data-component="floor-tile"]) {
      grid-column: auto;
      grid-row: auto;
    }
  }
</style>
