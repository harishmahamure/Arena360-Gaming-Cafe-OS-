<script lang="ts">
  import { Button, GroupChip, InlinePin, StatusBadge, TextField, TierLock, t } from "@arena/ui";
  import type {
    DeviceStatus,
    HubSnapshot,
    IntentPayload,
    SessionStaff,
    SnapshotDevice,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    elevate,
    hasPermission,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { SNAPSHOT_DEPENDENCY } from "../../../lib/snapshot";

  type Job = { payload: IntentPayload; rev?: number; reason?: "other" };

  const KINDS: { id: "console" | "vr" | "sim"; label: MessageKey }[] = [
    { id: "console", label: "hub.devices.kind.console" },
    { id: "vr", label: "hub.devices.kind.vr" },
    { id: "sim", label: "hub.devices.kind.sim" },
  ];
  const REFUSALS: Record<string, MessageKey> = {
    STALE: "hub.settings.branch.stale",
    GROUP_KIND_MISMATCH: "hub.groups.kind_mismatch",
    PERMISSION_DENIED: "hub.refusal.permission",
  };

  const BADGE: Record<
    DeviceStatus,
    { state: "free" | "offline" | "in-session" | "reserved" | "maintenance"; label: MessageKey }
  > = {
    online: { state: "free", label: "hub.floor.online" },
    offline: { state: "offline", label: "hub.floor.offline" },
    in_session: { state: "in-session", label: "ui.status.in_session" },
    reserved: { state: "reserved", label: "ui.status.reserved" },
    maintenance: { state: "maintenance", label: "ui.status.maintenance" },
  };

  const snapshot = $derived(
    (page.data as { hub?: { ok: boolean; snapshot?: HubSnapshot } }).hub?.snapshot ?? null,
  );
  const staff = $derived.by(() => {
    const session = (page.data as { session?: { state: string; staff?: SessionStaff } }).session;
    return session?.state === "signed_in" ? (session.staff ?? null) : null;
  });
  const needs = (kind: string) =>
    !hasPermission(staff, kind === "device.revoke" ? "device.remote" : "device.manage");
  const groups = $derived(snapshot?.groups ?? []);
  const manual = $derived((snapshot?.devices ?? []).filter((d) => d.manual));
  const all = $derived(snapshot?.devices ?? []);
  const fits = (kind: string) => (kind === "sim" ? "other" : kind);

  let editing: SnapshotDevice | null = $state(null);
  let kind: "console" | "vr" | "sim" = $state("console");
  let label = $state("");
  let groupId = $state("");
  let tv = $state("");
  let busy = $state(false);
  let form: HTMLFormElement | undefined = $state();
  let approving: Job | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let limit = $state(false);
  let labelError: MessageKey | null = $state(null);
  let tvError: MessageKey | null = $state(null);
  let maint: SnapshotDevice | null = $state(null);
  let note = $state("");
  let noteError: MessageKey | null = $state(null);
  let noteField: HTMLElement | undefined = $state();

  function openMaintenance(device: SnapshotDevice | null) {
    maint = device;
    note = device?.maintenance_note ?? "";
    noteError = null;
    if (device) void tick().then(() => noteField?.querySelector("input")?.focus());
  }

  function setMaintenance(event: SubmitEvent) {
    event.preventDefault();
    if (!maint) return;
    void run({
      payload: { kind: "device.maintenance", device_id: maint.device_id, on: true, note },
      rev: maint.meta_rev ?? undefined,
    });
  }

  function reset(device: SnapshotDevice | null = null) {
    editing = device;
    if (device) void tick().then(() => form?.querySelector("input")?.focus());
    kind = (device?.kind as typeof kind | null | undefined) ?? "console";
    label = device?.label ?? "";
    groupId = device?.group_id ?? "";
    tv = device?.tv_number ? String(device.tv_number) : "";
    labelError = tvError = null;
  }

  async function run(job: Job, ticket?: string) {
    if (!ticket && needs(job.payload.kind)) {
      approvalPrompt = "ui.pin.prompt";
      approving = job;
      return;
    }
    if (busy) return;
    busy = true;
    status = null;
    limit = false;
    labelError = tvError = noteError = null;
    const outcome = await submitIntent(job.payload, newIdempotencyKey(), job.rev, {
      elevationTicket: ticket,
      reason: job.reason,
    }).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      status = { tone: "ok", message: "hub.settings.saved" };
      if (job.payload.kind === "device.register") reset();
      if (job.payload.kind === "device.maintenance" && maint?.device_id === job.payload.device_id)
        openMaintenance(null);
      return void (await invalidate(SNAPSHOT_DEPENDENCY));
    }
    const error = outcome.kind === "error" ? outcome.error : null;
    if (error?.code === "PIN_REQUIRED") {
      approvalPrompt = "ui.pin.again";
      approving = job;
      return;
    }
    if (error?.code === "DEVICE_LIMIT") return void (limit = true);
    if (error?.code === "VALIDATION_FAILED" && error.field === "label") {
      return void (labelError = "hub.devices.label_invalid");
    }
    if (error?.code === "VALIDATION_FAILED" && error.field === "note") {
      return void (noteError = "hub.devices.note_invalid");
    }
    if (error?.code === "VALIDATION_FAILED" && error.field === "tv_number") {
      return void (tvError = "hub.devices.tv_invalid");
    }
    const gone =
      error?.code === "VALIDATION_FAILED" &&
      ["device_id", "base_rev", "group_id"].includes(error.field ?? "");
    status = {
      tone: "refusal",
      message: gone
        ? "hub.settings.branch.stale"
        : (error && REFUSALS[error.code]) || "hub.settings.failed",
    };
    if (gone || error?.code === "STALE") await invalidate(SNAPSHOT_DEPENDENCY);
  }

  async function approve(pin: string) {
    const job = approving;
    if (!job) return { ok: false, message: "ui.pin.failed" } as const;
    const outcome = await elevate([{ intent_kind: job.payload.kind }], pin);
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    approving = null;
    void run(job, outcome.ticket);
    return { ok: true } as const;
  }

  function save(event: SubmitEvent) {
    event.preventDefault();
    const tv_number =
      kind === "console" ? (/^\d{1,3}$/.test(tv.trim()) ? Number(tv) : 0) : undefined;
    const payload: IntentPayload = {
      kind: "device.register",
      class: "paired",
      device_kind: kind,
      label,
      token_hash: "",
      tv_number,
      ...(editing ? { device_id: editing.device_id } : { group_id: groupId || undefined }),
    };
    void run({ payload, rev: editing?.meta_rev ?? undefined });
  }
</script>

<section class="devices" aria-labelledby="settings-devices-title">
  <h2 class="type-hub-title" id="settings-devices-title">{t("hub.devices.title")}</h2>
  <p class="intro type-hub-body">{t("hub.devices.intro")}</p>

  <form class="form" bind:this={form} onsubmit={save} data-testid="device-form">
    <label class="field type-hub-label">
      <span>{t("hub.devices.kind")}</span>
      <select
        class="ui-interactive select"
        bind:value={kind}
        onchange={() => (groupId = "")}
        disabled={!!editing}
      >
        {#each KINDS as k (k.id)}<option value={k.id}>{t(k.label)}</option>{/each}
      </select>
    </label>
    <TextField label="hub.devices.label" bind:value={label} error={labelError} required />
    {#if kind === "console"}
      <TextField
        label="hub.devices.tv"
        bind:value={tv}
        error={tvError}
        inputmode="numeric"
        required
      />
    {/if}
    {#if !editing}
      <label class="field type-hub-label">
        <span>{t("hub.devices.group")}</span>
        <select class="ui-interactive select" bind:value={groupId}>
          <option value="">{t("hub.devices.no_group")}</option>
          {#each groups.filter((g) => g.kind === fits(kind)) as g (g.id)}
            <option value={g.id}>{g.name}</option>
          {/each}
        </select>
      </label>
    {/if}
    <div class="actions">
      <Button label={editing ? "hub.devices.save" : "hub.devices.add"} type="submit" />
      {#if editing}<Button
          label="hub.devices.cancel"
          variant="secondary"
          onclick={() => reset()}
        />{/if}
    </div>
  </form>

  <p
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="device-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
  </p>
  {#if limit}
    <div role="alert" data-testid="device-limit">
      <p class="type-hub-label">{t("hub.devices.limit")}</p>
      <TierLock tier="Basic" />
    </div>
  {/if}

  {#if approving}
    <div class="approval" data-testid="device-approval">
      <p class="type-hub-label">{t("hub.devices.approves")}</p>
      <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = null)} />
    </div>
  {/if}

  <h3 class="type-hub-title">{t("hub.devices.status_title")}</h3>
  <p class="intro type-hub-body">{t("hub.devices.status_intro")}</p>
  {#if maint}
    <form
      class="form"
      bind:this={noteField}
      onsubmit={setMaintenance}
      data-testid="maintenance-form"
    >
      <TextField label="hub.devices.note" bind:value={note} error={noteError} />
      <div class="actions">
        <Button label="hub.devices.maintenance_confirm" type="submit" />
        <Button
          label="hub.devices.cancel_maintenance"
          variant="secondary"
          onclick={() => openMaintenance(null)}
        />
      </div>
    </form>
  {/if}
  {#if all.length === 0}
    <p class="type-hub-body">{t("hub.floor.no_devices")}</p>
  {:else}
    <ul class="rows" data-testid="status-list">
      {#each all as d (d.device_id)}
        {@const badge = BADGE[d.status ?? "offline"] ?? BADGE.offline}
        <li class="row" data-testid="status-row" data-device-id={d.device_id}>
          <span class="type-hub-title">{d.label}</span>
          <StatusBadge state={badge.state} label={badge.label} />
          {#if d.status === "maintenance" && d.maintenance_note}<span
              class="type-hub-label"
              data-testid="maintenance-note">{d.maintenance_note}</span
            >{/if}
          <div class="actions">
            {#if d.status === "maintenance"}
              <button
                type="button"
                class="ui-interactive btn type-hub-label"
                onclick={() =>
                  void run({
                    payload: { kind: "device.maintenance", device_id: d.device_id, on: false },
                    rev: d.meta_rev ?? undefined,
                  })}>{t("hub.devices.return_to_service", { name: d.label })}</button
              >
            {/if}
            <button
              type="button"
              class="ui-interactive btn type-hub-label"
              onclick={() => openMaintenance(d)}
              >{t(
                d.status === "maintenance" ? "hub.devices.edit_note" : "hub.devices.maintenance",
                {
                  name: d.label,
                },
              )}</button
            >
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  <h3 class="type-hub-title">{t("hub.devices.list")}</h3>
  {#if manual.length === 0}
    <p class="type-hub-body">{t("hub.devices.empty")}</p>
  {:else}
    <ul class="rows" data-testid="device-list">
      {#each manual as d (d.device_id)}
        {@const group = groups.find((g) => g.id === d.group_id)}
        <li class="row" data-testid="device-row" data-device-id={d.device_id}>
          <span class="type-hub-title">{d.label}</span>
          {#if d.tv_number}<span class="type-hub-label"
              >{t("hub.floor.tv", { n: d.tv_number })}</span
            >{/if}
          {#if group}<GroupChip name={group.name} colour={group.colour} />{/if}
          <div class="actions">
            <button type="button" class="ui-interactive btn type-hub-label" onclick={() => reset(d)}
              >{t("hub.devices.edit", { name: d.label })}</button
            >
            <button
              type="button"
              class="ui-interactive btn type-hub-label"
              onclick={() =>
                void run({
                  payload: { kind: "device.revoke", device_id: d.device_id },
                  reason: "other",
                })}>{t("hub.devices.remove", { name: d.label })}</button
            >
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .devices,
  .form {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  h3,
  p {
    margin: 0;
  }

  .intro {
    color: var(--text-muted);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  .select {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
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

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .btn {
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--surface-2);
    color: var(--text);
    cursor: pointer;
  }

  .status {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }

  .approval {
    display: grid;
    gap: var(--space-2);
  }
</style>
