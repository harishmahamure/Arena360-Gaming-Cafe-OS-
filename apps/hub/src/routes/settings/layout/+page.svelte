<script lang="ts">
  import { Button, GroupChip, InlinePin, TextField, t } from "@arena/ui";
  import type {
    HubSnapshot,
    IntentPayload,
    SessionStaff,
    SnapshotGroup,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
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

  type Job = { payload: IntentPayload; rev?: number };

  const KINDS: { id: string; label: MessageKey }[] = [
    { id: "pc", label: "hub.groups.kind.pc" },
    { id: "console", label: "hub.groups.kind.console" },
    { id: "vr", label: "hub.groups.kind.vr" },
    { id: "other", label: "hub.groups.kind.other" },
  ];
  const COLOURS: { id: string; label: MessageKey }[] = [
    { id: "teal", label: "hub.groups.colour.teal" },
    { id: "orange", label: "hub.groups.colour.orange" },
    { id: "lime", label: "hub.groups.colour.lime" },
    { id: "sand", label: "hub.groups.colour.sand" },
    { id: "coral", label: "hub.groups.colour.coral" },
    { id: "silver", label: "hub.groups.colour.silver" },
  ];
  const REFUSALS: Record<string, MessageKey> = {
    STALE: "hub.settings.branch.stale",
    GROUP_NOT_EMPTY: "hub.groups.not_empty",
    GROUP_KIND_MISMATCH: "hub.groups.kind_mismatch",
    PERMISSION_DENIED: "hub.refusal.permission",
  };

  const snapshot = $derived(
    (page.data as { hub?: { ok: boolean; snapshot?: HubSnapshot } }).hub?.snapshot ?? null,
  );
  const staff = $derived.by(() => {
    const session = (page.data as { session?: { state: string; staff?: SessionStaff } }).session;
    return session?.state === "signed_in" ? (session.staff ?? null) : null;
  });
  const needsApproval = $derived(!hasPermission(staff, "device.manage"));
  const groups = $derived(snapshot?.groups ?? []);
  const devices = $derived((snapshot?.devices ?? []).filter((d) => d.class === "paired" && d.kind));

  let editing: SnapshotGroup | null = $state(null);
  let name = $state("");
  let kind = $state("pc");
  let colour = $state("teal");
  let order = $state("1");
  let approving: Job | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let nameError: MessageKey | null = $state(null);

  function reset(group: SnapshotGroup | null = null) {
    editing = group;
    name = group?.name ?? "";
    kind = group?.kind ?? "pc";
    colour = group?.colour ?? "teal";
    order = String(group?.sort_order ?? Math.max(0, ...groups.map((g) => g.sort_order)) + 1);
    nameError = null;
  }

  async function run(job: Job, ticket?: string) {
    if (!ticket && needsApproval) {
      approvalPrompt = "ui.pin.prompt";
      approving = job;
      return;
    }
    status = null;
    nameError = null;
    const outcome = await submitIntent(job.payload, newIdempotencyKey(), job.rev, {
      elevationTicket: ticket,
    });
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      status = { tone: "ok", message: "hub.settings.saved" };
      if (job.payload.kind === "group.upsert") reset();
      return void (await invalidate(SNAPSHOT_DEPENDENCY));
    }
    const error = outcome.kind === "error" ? outcome.error : null;
    if (error?.code === "PIN_REQUIRED") {
      approvalPrompt = "ui.pin.again";
      approving = job;
      return;
    }
    if (error?.code === "VALIDATION_FAILED" && error.field === "name") {
      nameError = "hub.groups.name_invalid";
      return;
    }
    const gone =
      error?.code === "VALIDATION_FAILED" && ["id", "base_rev"].includes(error.field ?? "");
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
    const sort_order = Number.parseInt(order, 10) || 0;
    const payload: IntentPayload = editing
      ? { kind: "group.upsert", id: editing.id, name, group_kind: kind, colour, sort_order }
      : { kind: "group.upsert", name, group_kind: kind, colour, sort_order };
    void run({ payload, rev: editing?.rev });
  }

  async function move(deviceId: string, groupId: string | null) {
    const d = devices.find((x) => x.device_id === deviceId);
    const g = groups.find((x) => x.id === groupId);
    if (!d || (d.group_id ?? null) === groupId || (g && !fits(d.kind, g))) return;
    await run({
      payload: { kind: "device.assign", device_id: deviceId, group_id: groupId },
      rev: d.meta_rev ?? undefined,
    });
  }

  const fits = (deviceKind: string | null | undefined, group: SnapshotGroup) =>
    (deviceKind === "sim" ? "other" : deviceKind) === group.kind;
  const kindLabel = (id: string) =>
    t(KINDS.find((k) => k.id === id)?.label ?? "hub.groups.kind.other");
</script>

<section class="layout" aria-labelledby="settings-layout-title">
  <h2 class="type-hub-title" id="settings-layout-title">{t("hub.groups.title")}</h2>
  <p class="intro type-hub-body">{t("hub.groups.intro")}</p>

  <form class="form" onsubmit={save} data-testid="group-form">
    <TextField label="hub.groups.name" bind:value={name} error={nameError} required />
    <label class="field type-hub-label">
      <span>{t("hub.groups.kind")}</span>
      <select class="ui-interactive select" bind:value={kind} disabled={!!editing}>
        {#each KINDS as k (k.id)}<option value={k.id}>{t(k.label)}</option>{/each}
      </select>
    </label>
    <fieldset class="swatches">
      <legend class="type-hub-label">{t("hub.groups.colour")}</legend>
      {#each COLOURS as c (c.id)}
        <label class="swatch type-hub-label">
          <input type="radio" name="colour" value={c.id} bind:group={colour} />
          <GroupChip name={t(c.label)} colour={c.id} />
        </label>
      {/each}
    </fieldset>
    <TextField label="hub.groups.order" bind:value={order} inputmode="numeric" />
    <div class="actions">
      <Button label={editing ? "hub.groups.save" : "hub.groups.add"} type="submit" />
      {#if editing}<Button
          label="hub.groups.cancel"
          variant="secondary"
          onclick={() => reset()}
        />{/if}
    </div>
  </form>

  <p
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="group-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
  </p>

  {#if approving}
    <div class="approval" data-testid="group-approval">
      <p class="type-hub-label">{t("hub.groups.approves")}</p>
      <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = null)} />
    </div>
  {/if}

  <p class="hint type-hub-label">{t("hub.groups.drag_hint")}</p>
  {#if groups.length === 0}
    <p class="type-hub-body">{t("hub.groups.empty")}</p>
  {:else}
    <ul class="rows" data-testid="group-list">
      {#each groups as g (g.id)}
        <li class="row" data-testid="group-row" data-group-id={g.id}>
          <div
            class="drop"
            role="presentation"
            ondragover={(e) => e.preventDefault()}
            ondrop={(e) => {
              e.preventDefault();
              void move(e.dataTransfer?.getData("text/plain") ?? "", g.id);
            }}
          >
            <GroupChip name={g.name} colour={g.colour} />
            <span class="type-hub-label">{kindLabel(g.kind)}</span>
          </div>
          <div class="actions">
            <button type="button" class="ui-interactive btn type-hub-label" onclick={() => reset(g)}
              >{t("hub.groups.edit", { name: g.name })}</button
            >
            <button
              type="button"
              class="ui-interactive btn type-hub-label"
              onclick={() => void run({ payload: { kind: "group.delete", id: g.id }, rev: g.rev })}
              >{t("hub.groups.delete", { name: g.name })}</button
            >
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  <h3 class="type-hub-title">{t("hub.groups.devices")}</h3>
  {#if devices.length === 0}
    <p class="type-hub-body">{t("hub.groups.no_devices")}</p>
  {:else}
    <ul class="rows" data-testid="group-devices">
      {#each devices as d (d.device_id)}
        <li
          class="row"
          draggable="true"
          ondragstart={(e) => e.dataTransfer?.setData("text/plain", d.device_id)}
          data-testid="group-device"
          data-device-id={d.device_id}
        >
          <span class="type-hub-title">{d.label}</span>
          <label class="field type-hub-label">
            <span>{t("hub.groups.move")}</span>
            <select
              class="ui-interactive select"
              value={d.group_id ?? ""}
              onchange={async (e) => {
                const el = e.currentTarget;
                await move(d.device_id, el.value || null);
                el.value = d.group_id ?? "";
              }}
            >
              <option value="">{t("hub.groups.unassigned")}</option>
              {#each groups.filter((g) => fits(d.kind, g)) as g (g.id)}
                <option value={g.id}>{g.name}</option>
              {/each}
            </select>
          </label>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .layout,
  .form {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  h3,
  p {
    margin: 0;
  }

  .intro,
  .hint {
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

  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    border: 0;
  }

  .swatch {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
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

  .drop {
    display: flex;
    align-items: center;
    gap: var(--space-3);
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
