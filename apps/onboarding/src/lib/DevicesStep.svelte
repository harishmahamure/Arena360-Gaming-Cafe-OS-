<script lang="ts">
  import {
    Button,
    GroupChip,
    StatusBadge,
    TextField,
    TierLock,
    focusFirstInvalid,
    t,
  } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SetupDevicesView } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import { getDevices, getWizard, sendDeviceCommand, type ApiFailure } from "./api";
  import SetupCodeNotice from "./SetupCodeNotice.svelte";
  import SetupStartPanel from "./SetupStartPanel.svelte";
  import { age, fits, overlay, type Refusal } from "./devices";
  import { validText } from "./validate";
  import { advance, STEP_NAMES, wizard } from "./wizard.svelte";

  const KINDS: [string, MessageKey][] = [
    ["pc", "hub.groups.kind.pc"],
    ["console", "hub.groups.kind.console"],
    ["vr", "hub.groups.kind.vr"],
    ["other", "hub.groups.kind.other"],
  ];
  const COLOURS: [string, MessageKey][] = [
    ["teal", "hub.groups.colour.teal"],
    ["orange", "hub.groups.colour.orange"],
    ["lime", "hub.groups.colour.lime"],
    ["sand", "hub.groups.colour.sand"],
    ["coral", "hub.groups.colour.coral"],
    ["silver", "hub.groups.colour.silver"],
  ];
  const STATUSES: Record<string, MessageKey> = {
    online: "onboarding.devices.status.online",
    offline: "onboarding.devices.status.offline",
    in_session: "onboarding.devices.status.in_session",
    reserved: "onboarding.devices.status.reserved",
    maintenance: "onboarding.devices.status.maintenance",
  };
  const EXTRA: [string, MessageKey][] = [
    ["console", "hub.devices.kind.console"],
    ["vr", "hub.devices.kind.vr"],
    ["sim", "hub.devices.kind.sim"],
  ];
  const REASONS: Record<string, MessageKey> = {
    GROUP_KIND_MISMATCH: "onboarding.devices.reason.kind_mismatch",
    VALIDATION_FAILED: "onboarding.devices.reason.invalid",
  };
  const POLL_MS = 5000;

  let view: SetupDevicesView | null = $state(null);
  let failed: string | null = $state(null);
  let dismissed: string[] = $state([]);
  let busy = $state(false);
  let name = $state("");
  let kind = $state("pc");
  let colour = $state("teal");
  let nameError: MessageKey | null = $state(null);
  let label = $state("");
  let extraKind = $state("console");
  let extraGroup = $state("");
  let tv = $state("");
  let labelError: MessageKey | null = $state(null);
  let tvError: MessageKey | null = $state(null);
  let groupForm: HTMLFormElement | undefined = $state();
  let deviceForm: HTMLFormElement | undefined = $state();

  const shown = $derived(view ? overlay(view) : null);
  const unassigned = $derived(shown?.devices.filter((d) => !d.group_id) ?? []);
  const fitting = $derived(shown?.groups.filter((g) => !g.waiting && fits(extraKind, g)) ?? []);
  const ago = $derived(view ? age(view) : null);
  const open = (r: Refusal) => !dismissed.includes(r.key);
  const formRefusals = (k: string) => shown?.refusals.filter((r) => r.kind === k && open(r)) ?? [];
  const deviceRefusals = (id: string) =>
    shown?.refusals.filter((r) => r.subject === id && open(r)) ?? [];

  function reason(r: Refusal): MessageKey {
    return REASONS[r.error.code] ?? "onboarding.devices.reason.other";
  }

  function failure(f: ApiFailure): string {
    if (f.error.code === "STEP_NOT_READY") return t("onboarding.staff.error.not_ready");
    return t(f.offline ? "onboarding.error.offline" : "onboarding.error.generic");
  }

  async function refresh() {
    const res = await getDevices();
    if (!res.ok) {
      if (!view) failed = failure(res);
      return;
    }
    failed = null;
    view = res.data;
    if (wizard.state?.current !== "devices" || !res.data.setup?.devices.some((d) => d.group_id)) {
      return;
    }
    const next = await getWizard();
    if (next.ok && next.data.current !== "devices") await advance(next.data);
  }

  async function send(commandKind: string, payload: unknown): Promise<boolean> {
    if (busy) return false;
    busy = true;
    dismissed = shown?.refusals.map((r) => r.key) ?? [];
    const res = await sendDeviceCommand(commandKind, payload);
    busy = false;
    if (!res.ok) {
      failed = failure(res);
      return false;
    }
    failed = null;
    await refresh();
    return true;
  }

  async function addGroup(event: SubmitEvent) {
    event.preventDefault();
    nameError = validText(name) ? null : "onboarding.devices.error.name";
    if (nameError) {
      if (groupForm) focusFirstInvalid(groupForm);
      return;
    }
    const sort_order = shown?.groups.length ?? 0;
    const ok = await send("group.upsert", {
      name: name.trim(),
      group_kind: kind,
      colour,
      sort_order,
    });
    if (ok) name = "";
  }

  async function move(deviceId: string, groupId: string | null) {
    const d = shown?.devices.find((x) => x.id === deviceId);
    const g = shown?.groups.find((x) => x.id === groupId);
    if (!d || d.waiting || (d.group_id ?? null) === groupId || g?.waiting) return;
    if (g && !fits(d.kind, g)) return;
    await send("device.assign", { device_id: deviceId, group_id: groupId });
  }

  async function addDevice(event: SubmitEvent) {
    event.preventDefault();
    labelError = validText(label) ? null : "onboarding.devices.error.label";
    const number = Number(tv);
    tvError =
      extraKind !== "console" || (/^\d{1,3}$/.test(tv) && number >= 1)
        ? null
        : "onboarding.devices.error.tv";
    if (labelError || tvError) {
      if (deviceForm) focusFirstInvalid(deviceForm);
      return;
    }
    const ok = await send("device.register", {
      class: "paired",
      device_kind: extraKind,
      label: label.trim(),
      ...(extraKind === "console" ? { tv_number: number } : {}),
      ...(extraGroup ? { group_id: extraGroup } : {}),
    });
    if (ok) {
      label = "";
      tv = "";
    }
  }

  onMount(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  });
</script>

<h1 tabindex="-1">{t(STEP_NAMES.devices)}</h1>
<SetupCodeNotice />
<p class="intro">{t("onboarding.devices.intro")}</p>
<SetupStartPanel />

{#if view && !view.hub_online}
  <p class="offline" role="status" data-testid="hub-offline">
    <StatusBadge state="offline" label="onboarding.devices.hub_offline" />
    {#if ago !== null}
      <span>
        {ago < 1 ? t("onboarding.devices.as_of_now") : t("onboarding.devices.as_of", { n: ago })}
      </span>
    {/if}
  </p>
{/if}

{#if failed}
  <p class="form-error" role="alert"><span aria-hidden="true">⚠</span><span>{failed}</span></p>
{/if}

<section aria-labelledby="devices-unassigned">
  <h2 id="devices-unassigned">{t("onboarding.devices.unassigned")}</h2>
  {#if view && unassigned.length === 0}
    <p class="muted">{t("onboarding.devices.unassigned_empty")}</p>
  {/if}
  <ul class="rows" data-testid="unassigned-list">
    {#each unassigned as d (d.id)}
      {@render device(d)}
    {/each}
  </ul>
  <div
    class="drop"
    role="presentation"
    ondragover={(e) => e.preventDefault()}
    ondrop={(e) => {
      e.preventDefault();
      void move(e.dataTransfer?.getData("text/plain") ?? "", null);
    }}
  ></div>
</section>

<section aria-labelledby="devices-groups">
  <h2 id="devices-groups">{t("onboarding.devices.groups")}</h2>
  <p class="muted">{t("onboarding.devices.drag_hint")}</p>
  <ul class="rows" data-testid="group-list">
    {#each shown?.groups ?? [] as g (g.id)}
      <li class="group" data-testid="group-{g.id}">
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
          <span class="muted"
            >{t(KINDS.find(([k]) => k === g.kind)?.[1] ?? "hub.groups.kind.other")}</span
          >
          {#if g.waiting}<span class="state">{t("onboarding.devices.waiting")}</span>{/if}
        </div>
        <ul class="rows">
          {#each shown?.devices.filter((d) => d.group_id === g.id) ?? [] as d (d.id)}
            {@render device(d)}
          {/each}
        </ul>
      </li>
    {/each}
  </ul>
  {#each formRefusals("group.upsert") as r (r.key)}
    <p class="form-error" role="alert">⚠ {t(reason(r))}</p>
  {/each}
  <form bind:this={groupForm} onsubmit={addGroup} novalidate data-testid="group-form">
    <h3>{t("onboarding.devices.group.title")}</h3>
    <TextField label="hub.groups.name" bind:value={name} autocomplete="off" error={nameError} />
    <label class="field">
      <span>{t("hub.groups.kind")}</span>
      <select class="ui-interactive select" bind:value={kind}>
        {#each KINDS as [id, key] (id)}<option value={id}>{t(key)}</option>{/each}
      </select>
    </label>
    <fieldset class="swatches">
      <legend>{t("hub.groups.colour")}</legend>
      {#each COLOURS as [c, key] (c)}
        <label class="swatch">
          <input type="radio" name="colour" value={c} bind:group={colour} />
          <GroupChip name={t(key)} colour={c} />
        </label>
      {/each}
    </fieldset>
    <div class="actions">
      <Button
        type="submit"
        label="hub.groups.add"
        disabled={busy}
        disabledReason="onboarding.devices.sending"
      />
    </div>
  </form>
</section>

<section aria-labelledby="devices-add">
  <h2 id="devices-add">{t("hub.devices.title")}</h2>
  {#each formRefusals("device.register") as r (r.key)}
    <p class="form-error" role="alert">⚠ {t(reason(r))}</p>
    {#if r.error.code === "DEVICE_LIMIT"}<TierLock tier="Basic" />{/if}
  {/each}
  <form bind:this={deviceForm} onsubmit={addDevice} novalidate data-testid="device-form">
    <label class="field">
      <span>{t("hub.devices.kind")}</span>
      <select
        class="ui-interactive select"
        bind:value={extraKind}
        onchange={() => (extraGroup = "")}
      >
        {#each EXTRA as [id, key] (id)}<option value={id}>{t(key)}</option>{/each}
      </select>
    </label>
    <TextField label="hub.devices.label" bind:value={label} autocomplete="off" error={labelError} />
    <label class="field">
      <span>{t("hub.devices.group")}</span>
      <select class="ui-interactive select" bind:value={extraGroup}>
        <option value="">{t("hub.devices.no_group")}</option>
        {#each fitting as g (g.id)}<option value={g.id}>{g.name}</option>{/each}
      </select>
    </label>
    {#if extraKind === "console"}
      <TextField label="hub.devices.tv" bind:value={tv} inputmode="numeric" error={tvError} />
    {/if}
    <div class="actions">
      <Button
        type="submit"
        label="hub.devices.add"
        disabled={busy}
        disabledReason="onboarding.devices.sending"
      />
    </div>
  </form>
</section>

{#snippet device(d: NonNullable<typeof shown>["devices"][number])}
  <li
    class="device"
    draggable="true"
    ondragstart={(e) => e.dataTransfer?.setData("text/plain", d.id)}
    data-testid="device-{d.id}"
  >
    <span class="name">{d.label}</span>
    <span class="muted"
      >{t(
        KINDS.find(([k]) => k === (d.kind === "sim" ? "other" : d.kind))?.[1] ??
          "hub.groups.kind.other",
      )}</span
    >
    <span class="state">
      {d.waiting ? t("onboarding.devices.waiting") : t(STATUSES[d.status])}
    </span>
    <label class="field">
      <span>{t("onboarding.devices.move")}</span>
      <select
        class="ui-interactive select"
        value={d.group_id ?? ""}
        disabled={d.waiting}
        onchange={async (e) => {
          const el = e.currentTarget;
          await move(d.id, el.value || null);
          el.value = d.group_id ?? "";
        }}
      >
        <option value="">{t("onboarding.devices.unassigned")}</option>
        {#each shown?.groups.filter((g) => !g.waiting && fits(d.kind, g)) ?? [] as g (g.id)}
          <option value={g.id}>{g.name}</option>
        {/each}
      </select>
    </label>
    {#each deviceRefusals(d.id) as r (r.key)}
      <p class="form-error" role="alert">⚠ {t(reason(r))}</p>
    {/each}
  </li>
{/snippet}

<style>
  .intro,
  .muted {
    margin: 0 0 var(--space-3);
    color: var(--text-muted);
    line-height: var(--type-hub-label-line-height);
  }

  section {
    margin-top: var(--space-5);
  }

  h2 {
    margin: 0 0 var(--space-3);
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  h3 {
    margin: 0;
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .offline {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0 0 var(--space-4);
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0 0 var(--space-3);
    padding: 0;
    list-style: none;
  }

  .group,
  .device {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
  }

  .device {
    cursor: grab;
    width: 100%;
    box-sizing: border-box;
  }

  .group .rows {
    width: 100%;
    margin: 0;
  }

  .drop {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
  }

  .name {
    font-weight: var(--type-hub-title-weight);
  }

  .state {
    font-weight: var(--type-hub-title-weight);
    font-size: var(--type-hub-label-size);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-width: 32rem;
    margin-top: var(--space-4);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .select {
    --ui-bg: var(--text-field-background);
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
  }

  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-4);
    margin: 0;
    padding: 0;
    border: 0;
  }

  .swatches legend {
    margin-bottom: var(--space-2);
    padding: 0;
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .swatch {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
  }

  .swatch input {
    width: 1.5rem;
    height: 1.5rem;
    margin: 0;
    accent-color: var(--accent);
  }

  .form-error {
    display: flex;
    gap: var(--space-2);
    margin: 0;
    color: var(--text-field-error-text);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
