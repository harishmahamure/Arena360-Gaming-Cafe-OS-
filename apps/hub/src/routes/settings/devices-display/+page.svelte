<script lang="ts">
  import { Button, InlinePin, t } from "@arena/ui";
  import type { HubSnapshot, SessionStaff } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    dropElevation,
    elevate,
    hasPermission,
    loadSettings,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { branchLocale } from "../../../lib/locale.svelte";
  import { shortFingerprint } from "../../../lib/snapshot";
  import PairKioskPanel from "../../../lib/PairKioskPanel.svelte";

  // Settings → Devices & display (Story 1.21): high contrast per device, set by a manager.
  // "This Hub" is `display.hc.hub`; each paired device is `display.hc.<device_id>`. Each switch
  // saves its own row with that row's `base_rev`; a Counter's change needs a manager's PIN in
  // place. There is no light theme and no player-facing control.

  const HUB_KEY = "display.hc.hub";
  const keyOf = (deviceId: string) => `display.hc.${deviceId}`; // i18n-ignore (a setting key)

  type Row = {
    key: string;
    label: string;
    testId: string;
    kitchen?: boolean;
    /** Restored from the cloud backup: pair it again with a code (Story 9.13). */
    again?: boolean;
  };
  type Server = { value: boolean; rev: number };
  type RowStatus = { tone: "ok" | "refusal"; message: MessageKey };

  const snapshot = $derived(
    (page.data as { hub?: { ok: boolean; snapshot?: HubSnapshot } }).hub?.snapshot ?? null,
  );
  const staff = $derived.by(() => {
    const session = (page.data as { session?: { state: string; staff?: SessionStaff } }).session;
    return session?.state === "signed_in" ? (session.staff ?? null) : null;
  });
  const needsApproval = $derived(!hasPermission(staff, "settings.edit"));
  const canPair = $derived(hasPermission(staff, "device.manage"));
  let pairOpen = $state(false);
  let pairButton: HTMLElement | undefined = $state();

  async function closePair() {
    pairOpen = false;
    await tick();
    pairButton?.querySelector("button")?.focus();
  }

  const lan = $derived(snapshot?.lan ?? null);
  type CopyStatus = { address: string; ok: boolean };
  let copyStatus: CopyStatus | null = $state(null);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  const ipNodes: Record<string, HTMLElement | undefined> = $state({});

  function showCopy(status: CopyStatus | null) {
    clearTimeout(copyTimer);
    copyStatus = status;
    if (status) copyTimer = setTimeout(() => (copyStatus = null), 4000);
  }

  function selectAddress(address: string) {
    const node = ipNodes[address];
    const selection = window.getSelection();
    if (!node || !selection) return;
    const range = document.createRange();
    range.selectNodeContents(node);
    selection.removeAllRanges();
    selection.addRange(range);
  }

  async function copyAddress(address: string) {
    try {
      await navigator.clipboard.writeText(address);
      showCopy({ address, ok: true });
    } catch {
      selectAddress(address);
      showCopy({ address, ok: false });
    }
  }

  $effect(() => {
    void lan?.addresses.join(" ");
    untrack(() => showCopy(null));
  });

  /** This Hub first, then every non-revoked paired device in the snapshot (it lists no revoked). */
  const rows: Row[] = $derived([
    { key: HUB_KEY, label: t("hub.settings.devices_display.this_hub"), testId: "hc-row-hub" },
    ...(snapshot?.devices ?? [])
      .filter((d) => d.class === "paired")
      .map((d) => ({
        key: keyOf(d.device_id),
        label: d.label,
        testId: `hc-row-${d.device_id}`, // i18n-ignore (a test id)
        again: d.pair_again === true,
      })),
    ...(snapshot?.kitchen_displays ?? []).map((d) => ({
      key: keyOf(d.id),
      label: d.name,
      testId: `hc-row-${d.id}`, // i18n-ignore (a test id)
      kitchen: true,
    })),
  ]);

  let server: Record<string, Server> = $state({});
  let loading = $state(true);
  let loadFailed = $state(false);
  /** The row being saved (one at a time), and the value it is being set to. */
  let saving: { key: string; value: boolean } | null = $state(null);
  /** The row whose change waits for a manager's PIN. */
  let approving: { key: string; value: boolean } | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let statuses: Record<string, RowStatus> = $state({});
  let pendingKey: { key: string; value: boolean; idem: string } | null = null;
  const switches: Record<string, HTMLInputElement | undefined> = $state({});

  /** Bumped by every load: only the latest one's answer is used. */
  let loadSeq = 0;

  async function load() {
    const seq = ++loadSeq;
    loading = true;
    try {
      const result = await loadSettings();
      if (seq !== loadSeq) return;
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (result.kind !== "ok") {
        loadFailed = true;
        return;
      }
      const next: Record<string, Server> = {};
      for (const s of result.settings) {
        if (s.key.startsWith("display.hc."))
          next[s.key] = { value: s.value === "true", rev: s.rev };
      }
      // A kitchen display's high contrast lives on its row, which the snapshot carries (Story 7.6).
      for (const d of snapshot?.kitchen_displays ?? []) {
        const key = keyOf(d.id);
        const held = server[key];
        next[key] = held && held.rev > d.rev ? held : { value: d.high_contrast, rev: d.rev };
      }
      server = next;
      loadFailed = !next[HUB_KEY];
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  /** Puts focus back on `key`'s switch once it is enabled again. */
  async function focusSwitch(key: string) {
    await tick();
    switches[key]?.focus();
  }

  /** What a row's switch shows: the value being saved or approved, else the Hub's. */
  function shown(key: string): boolean {
    if (saving?.key === key) return saving.value;
    if (approving?.key === key) return approving.value;
    return server[key]?.value ?? false;
  }

  function toggle(row: Row, value: boolean) {
    if (saving || approving || !server[row.key]) return;
    statuses = Object.fromEntries(Object.entries(statuses).filter(([k]) => k !== row.key));
    if (needsApproval) {
      approvalPrompt = "ui.pin.prompt";
      approving = { key: row.key, value };
      return;
    }
    void save(row.key, value);
  }

  /** The status a refused save shows, and whether it asks for the PIN again. */
  function refusal(code: string | null): { status: RowStatus | null; pinAgain: boolean } {
    if (code === "PIN_REQUIRED") return { status: null, pinAgain: true };
    if (code === "STALE")
      return { status: { tone: "refusal", message: "hub.settings.branch.stale" }, pinAgain: false };
    if (code === "PERMISSION_DENIED") {
      return { status: { tone: "refusal", message: "hub.refusal.permission" }, pinAgain: false };
    }
    return { status: { tone: "refusal", message: "hub.settings.failed" }, pinAgain: false };
  }

  async function save(key: string, value: boolean, ticket?: string) {
    const row = server[key];
    if (!row || row.value === value) {
      if (ticket) void dropElevation(ticket);
      return;
    }
    if (pendingKey?.key !== key || pendingKey.value !== value) {
      pendingKey = { key, value, idem: newIdempotencyKey() };
    }
    saving = { key, value };
    let outcome: Awaited<ReturnType<typeof submitIntent>>;
    try {
      outcome = await submitIntent(
        { kind: "settings.set", key, value: value ? "true" : "false" },
        pendingKey.idem,
        row.rev,
        { elevationTicket: ticket },
      );
    } finally {
      saving = null;
    }
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      pendingKey = null;
      server = { ...server, [key]: { value, rev: row.rev + 1 } };
      statuses = { ...statuses, [key]: { tone: "ok", message: "hub.settings.saved" } };
      void load();
      if (ticket) await focusSwitch(key);
      return;
    }
    const { status, pinAgain } = refusal(outcome.kind === "error" ? outcome.error.code : null);
    if (!pinAgain) pendingKey = null;
    if (status) statuses = { ...statuses, [key]: status };
    if (status?.message === "hub.settings.branch.stale") void load();
    if (pinAgain) {
      approvalPrompt = "ui.pin.again";
      approving = { key, value };
    } else if (ticket) await focusSwitch(key);
  }

  /** A manager's PIN approves exactly this one row's change. */
  async function approve(pin: string) {
    const target = approving;
    if (!target) return { ok: false, message: "ui.pin.failed" } as const;
    const outcome = await elevate([{ intent_kind: "settings.set", key: target.key }], pin);
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind === "unreachable")
      return { ok: false, message: "ui.pin.unreachable" } as const;
    if (outcome.kind === "error") return { ok: false, message: "ui.pin.failed" } as const;
    if (approving?.key !== target.key) {
      void dropElevation(outcome.ticket);
      return { ok: true } as const;
    }
    approving = null;
    void save(target.key, target.value, outcome.ticket);
    return { ok: true } as const;
  }

  // Another window or device changed settings: refresh.
  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      if (!saving) void load();
    });
  });

  // A kitchen display's row changed (a toggle elsewhere, a new display): follow the snapshot.
  $effect(() => {
    const list = snapshot?.kitchen_displays ?? [];
    untrack(() => {
      if (saving) return;
      const next = { ...server };
      let changed = false;
      for (const d of list) {
        const key = keyOf(d.id);
        const held = next[key];
        if (held && held.rev > d.rev) continue;
        if (held?.value === d.high_contrast && held.rev === d.rev) continue;
        next[key] = { value: d.high_contrast, rev: d.rev };
        changed = true;
      }
      if (changed) server = next;
    });
  });

  // A device paired or revoked (the snapshot was re-read): its row may be new.
  $effect(() => {
    const keys = rows.map((r) => r.key);
    untrack(() => {
      if (!loading && keys.some((k) => !server[k])) void load();
    });
  });

  onMount(() => {
    void load();
  });
</script>

<section class="devices-display" aria-labelledby="settings-devices-display-title">
  <h2 class="type-hub-title" id="settings-devices-display-title">
    {t("hub.settings.nav.devices_display")}
  </h2>
  <p class="intro type-hub-body">{t("hub.settings.devices_display.intro")}</p>
  {#if lan}
    <div class="row address" data-testid="hub-address">
      <p class="name type-hub-label">{t("hub.settings.devices_display.address")}</p>
      {#if lan.addresses.length > 0}
        <ul class="addresses">
          {#each lan.addresses as address (address)}
            <li class="line">
              <span
                class="ip type-hub-title"
                data-testid="hub-address-ip"
                bind:this={ipNodes[address]}>{address}</span
              >
              <button
                type="button"
                class="ui-interactive copy type-hub-label"
                aria-label={t("hub.settings.devices_display.copy_address", { address })}
                onclick={() => copyAddress(address)}
              >
                {t("hub.settings.devices_display.copy")}
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="note type-hub-label">{t("hub.settings.devices_display.address_none")}</p>
      {/if}
      {#if lan.network}
        <p class="note type-hub-label" data-testid="hub-address-network">
          {t("hub.settings.devices_display.network", { name: lan.network })}
        </p>
      {/if}
      <p class="note type-hub-label" data-testid="hub-address-fingerprint">
        {t("hub.settings.devices_display.fingerprint", {
          fingerprint: shortFingerprint(lan.fingerprint),
        })}
      </p>
      <p class="note type-hub-label">{t("hub.settings.devices_display.address_hint")}</p>
      <p class="status type-hub-label" role="status" data-testid="hub-address-status">
        {#if copyStatus?.ok}<span
            >{t("hub.settings.devices_display.copied", { address: copyStatus.address })}</span
          >{:else if copyStatus}<span>{t("hub.settings.devices_display.copy_manual")}</span>{/if}
      </p>
    </div>
  {/if}
  {#if canPair}
    <div class="pair" bind:this={pairButton}>
      {#if !pairOpen}
        <Button
          label="hub.settings.devices_display.pair"
          variant="primary"
          onclick={() => (pairOpen = true)}
        />
      {:else}
        <PairKioskPanel onclose={() => void closePair()} fingerprint={lan?.fingerprint ?? null} />
      {/if}
    </div>
  {/if}
  {#if loadFailed}
    <p class="status refusal type-hub-label" role="status" data-testid="hc-load-failed">
      <span aria-hidden="true">⚠</span>
      <span>{t("hub.settings.load_failed")}</span>
    </p>
  {:else}
    <ul class="rows" aria-label={t("hub.settings.devices_display.list")} data-testid="hc-rows">
      {#each rows as row (row.key)}
        {@const on = shown(row.key)}
        {@const known = server[row.key] !== undefined}
        <li class="row" data-testid={row.testId}>
          {#if row.kitchen}
            <p class="note type-hub-label">{t("hub.settings.devices_display.kitchen_displays")}</p>
          {/if}
          <div class="line">
            <span class="name type-hub-label">{row.label}</span>
            <label class="switch">
              <input
                bind:this={switches[row.key]}
                class="toggle ui-interactive"
                type="checkbox"
                role="switch"
                checked={on}
                aria-checked={on}
                aria-label={t("hub.settings.devices_display.hc_for", { device: row.label })}
                disabled={!known || saving !== null || approving !== null}
                onchange={(e) => toggle(row, e.currentTarget.checked)}
              />
              <span class="state type-hub-label" aria-hidden="true"
                >{t("hub.settings.devices_display.hc")} · {t(
                  on ? "hub.settings.devices_display.on" : "hub.settings.devices_display.off",
                )}</span
              >
            </label>
          </div>
          {#if row.again}
            <p class="note type-hub-label" data-testid="{row.testId}-pair-again">
              {t("hub.settings.devices_display.pair_again")}
            </p>
          {/if}
          {#if needsApproval && approving?.key !== row.key}
            <p class="note type-hub-label">{t("hub.settings.branch.needs_approval")}</p>
          {/if}
          <p
            class="status type-hub-label"
            class:refusal={statuses[row.key]?.tone === "refusal"}
            role="status"
            data-testid="{row.testId}-status"
          >
            {#if saving?.key === row.key}
              <span>{t("hub.settings.saving")}</span>
            {:else if statuses[row.key]}
              {#if statuses[row.key].tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
              <span>{t(statuses[row.key].message)}</span>
            {/if}
          </p>
          {#if approving?.key === row.key}
            <div class="approval" data-testid="hc-approval">
              <p class="note type-hub-label">
                {t("hub.settings.devices_display.approves", { device: row.label })}
              </p>
              <InlinePin
                message={approvalPrompt}
                onsubmit={approve}
                oncancel={() => {
                  approving = null;
                  void focusSwitch(row.key);
                }}
                returnFocusTo={switches[row.key] ?? null}
              />
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if rows.length === 1 && snapshot}
      <p class="note type-hub-label" data-testid="hc-no-devices">
        {t("hub.settings.devices_display.no_devices")}
      </p>
    {/if}
  {/if}
</section>

<style>
  .devices-display {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  .intro {
    margin: 0;
  }

  .intro {
    color: var(--text-muted);
  }

  .rows {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background-color: var(--surface);
  }

  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .name {
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .switch {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--space-touch-min);
    cursor: pointer;
  }

  /* A 44px-tall switch: the track is the control, the knob shows the state (and the text
     beside it says it, never colour alone). */
  .toggle {
    --ui-bg: var(--surface-2);
    appearance: none;
    position: relative;
    flex: none;
    width: calc(var(--space-touch-min) * 1.75);
    height: var(--space-touch-min);
    margin: 0;
    border: 2px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--ui-bg);
    cursor: pointer;
  }

  .toggle::after {
    content: "";
    position: absolute;
    top: 50%;
    left: var(--space-1);
    width: calc(var(--space-touch-min) - 2 * var(--space-2));
    height: calc(var(--space-touch-min) - 2 * var(--space-2));
    border-radius: var(--radius-full);
    background-color: var(--text-muted);
    transform: translateY(-50%);
  }

  .toggle:checked {
    --ui-bg: var(--accent);
    border-color: var(--accent);
  }

  .toggle:checked::after {
    left: auto;
    right: var(--space-1);
    background-color: var(--on-accent);
  }

  .toggle:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .state {
    color: var(--text);
  }

  .note,
  .status {
    margin: 0;
  }

  .note {
    color: var(--text);
  }

  .status {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
    color: var(--text);
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }

  .approval {
    display: grid;
    gap: var(--space-2);
  }

  .pair {
    display: grid;
    justify-items: start;
  }

  .addresses {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .ip {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .copy {
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--surface-2);
    color: var(--text);
    cursor: pointer;
  }
</style>
