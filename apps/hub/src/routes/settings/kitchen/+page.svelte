<script lang="ts">
  import { Button, InlinePin, formatDateTime, t } from "@arena/ui";
  import type {
    BluetoothAccess,
    FoundPrinter,
    HubSnapshot,
    KitchenDisplayView,
    PrinterErrorKind,
    PrinterSelected,
  } from "@arena/ui/generated";
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
  import PairKitchenPanel from "../../../lib/PairKitchenPanel.svelte";
  import { sessionStaff } from "../../../lib/operating";
  import {
    KEY_KOT_MODE,
    KEY_KOT_PAPER_MM,
    KEY_KOT_PRINTER,
    KEY_LICENCE_FLAGS,
    KITCHEN_KEYS,
    KOT_MODES,
    PAPER_WIDTHS,
    askBluetooth,
    findPrinters,
    openAppSettings,
    parseSelected,
    sameSelection,
    selectionOf,
    testKotPrint,
  } from "../../../lib/printer";

  type Row = { value: string; rev: number };

  const MODE_LABELS: Record<(typeof KOT_MODES)[number], MessageKey> = {
    receipt: "hub.settings.kitchen.mode.receipt",
    another: "hub.settings.kitchen.mode.another",
    none: "hub.settings.kitchen.mode.none",
  };
  const PAPER_LABELS: Record<(typeof PAPER_WIDTHS)[number], MessageKey> = {
    "58": "hub.settings.printer.paper_58",
    "80": "hub.settings.printer.paper_80",
  };
  const ERROR_KINDS: Record<PrinterErrorKind, MessageKey> = {
    refused: "hub.settings.printer.error.refused",
    timeout: "hub.settings.printer.error.timeout",
    unreachable: "hub.settings.printer.error.unreachable",
    spooler: "hub.settings.printer.error.spooler",
    bluetooth: "hub.settings.printer.error.bluetooth",
    bluetooth_permission: "hub.settings.printer.error.bluetooth_permission",
    other: "hub.settings.printer.error.other",
  };

  let rows: Record<string, Row> = $state({});
  let loading = $state(true);
  let loadFailed = $state(false);
  let busy = $state(false);
  let mode = $state("receipt");
  let paper = $state("80");
  let chosen: PrinterSelected | null = $state(null);
  let found: FoundPrinter[] | null = $state(null);
  let bluetooth: BluetoothAccess | null = $state(null);
  let usbSupported = $state(true);
  let askingBluetooth = $state(false);
  let ip = $state("");
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; text: string } | null = $state(null);
  let saveWrap: HTMLElement | undefined = $state();
  const saveButton = $derived(saveWrap?.querySelector("button") ?? null);

  const snapshot = $derived(
    (page.data as { hub?: { ok: boolean; snapshot?: HubSnapshot } }).hub?.snapshot ?? null,
  );
  const displays: KitchenDisplayView[] = $derived(snapshot?.kitchen_displays ?? []);
  const tz = $derived(snapshot?.locale?.tz);
  const staff = $derived(sessionStaff(page.data));
  const needsApproval = $derived(!hasPermission(staff, "settings.edit"));
  const canPair = $derived(hasPermission(staff, "device.manage"));
  let pairOpen = $state(false);
  let pairButton: HTMLElement | undefined = $state();
  /** The display being revoked, and the last revoke's outcome. */
  let revoking: string | null = $state(null);
  let revokeStatus: { tone: "ok" | "refusal"; text: string } | null = $state(null);
  const unlocked = $derived(
    (rows[KEY_LICENCE_FLAGS]?.value ?? "").split(",").some((f) => f.trim() === "kitchen"),
  );
  const saved = $derived(parseSelected(rows[KEY_KOT_PRINTER]?.value));
  const changes = $derived(
    (
      [
        [KEY_KOT_MODE, mode],
        [KEY_KOT_PAPER_MM, paper],
        [KEY_KOT_PRINTER, chosen ? JSON.stringify(chosen) : ""],
      ] as const
    ).filter(([k, v]) =>
      k === KEY_KOT_PRINTER
        ? chosen !== saved && !sameSelection(chosen, saved)
        : v !== rows[k]?.value,
    ),
  );
  const BLUETOOTH_NOTES: Record<Exclude<BluetoothAccess, "granted">, MessageKey> = {
    denied: "hub.settings.printer.bluetooth.denied",
    off: "hub.settings.printer.bluetooth.off",
    unavailable: "hub.settings.printer.bluetooth.unavailable",
  };
  const refuse = (message: MessageKey) => (status = { tone: "refusal", text: t(message) });

  async function load(quiet = false, keepEdits = false) {
    if (!quiet) loading = true;
    try {
      const result = await loadSettings();
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const next: Record<string, Row> = {};
      if (result.kind === "ok") {
        for (const s of result.settings) {
          if (KITCHEN_KEYS.includes(s.key)) next[s.key] = { value: s.value, rev: s.rev };
        }
      }
      loadFailed = KITCHEN_KEYS.some((k) => next[k] === undefined);
      if (loadFailed) return;
      rows = next;
      if (keepEdits) return;
      mode = next[KEY_KOT_MODE].value;
      paper = next[KEY_KOT_PAPER_MM].value;
      chosen = parseSelected(next[KEY_KOT_PRINTER].value);
    } finally {
      loading = false;
    }
  }

  async function closePair() {
    pairOpen = false;
    await tick();
    pairButton?.querySelector("button")?.focus();
  }

  async function revoke(display: KitchenDisplayView) {
    if (revoking) return;
    revoking = display.id;
    revokeStatus = null;
    try {
      const outcome = await submitIntent(
        { kind: "kds.revoke", display_id: display.id },
        newIdempotencyKey(),
        display.rev,
      );
      if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (outcome.kind === "applied") {
        revokeStatus = {
          tone: "ok",
          text: t("hub.settings.kitchen.revoked", { name: display.name }),
        };
      } else {
        const code = outcome.kind === "error" ? outcome.error.code : null;
        revokeStatus = {
          tone: "refusal",
          text:
            code === "PERMISSION_DENIED"
              ? t("hub.refusal.permission")
              : code === "STALE"
                ? t("hub.settings.branch.stale")
                : t("hub.settings.kitchen.revoke_failed"),
        };
      }
    } finally {
      revoking = null;
    }
  }

  function lastSeen(display: KitchenDisplayView): string {
    return display.last_seen
      ? t("hub.settings.kitchen.display_last_seen", { at: formatDateTime(display.last_seen, tz) })
      : t("hub.settings.kitchen.display_never_seen");
  }

  async function find() {
    busy = true;
    status = null;
    try {
      askingBluetooth = true;
      await askBluetooth();
      askingBluetooth = false;
      const r = await findPrinters();
      if (r.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (r.kind === "ok") {
        found = r.body.printers;
        bluetooth = r.body.bluetooth ?? null;
        usbSupported = r.body.usb_supported;
      } else refuse("hub.settings.printer.find_failed");
    } finally {
      askingBluetooth = false;
      busy = false;
    }
  }

  function addIp() {
    const text = ip.trim();
    if (!text) return refuse("hub.settings.printer.ip_empty");
    chosen = { kind: "lan", id_or_addr: text, name: text };
    ip = "";
  }

  async function save(ticket?: string) {
    if (busy || !changes.length) return ticket ? void dropElevation(ticket) : undefined;
    if (needsApproval && !ticket) {
      approvalPrompt = "ui.pin.prompt";
      approving = true;
      return;
    }
    busy = true;
    status = null;
    let pinAgain = false;
    try {
      for (const [key, value] of changes) {
        const outcome = await submitIntent(
          { kind: "settings.set", key, value },
          newIdempotencyKey(),
          rows[key].rev,
          { elevationTicket: ticket },
        );
        if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
        if (outcome.kind === "applied") continue;
        const e = outcome.kind === "error" ? outcome.error : null;
        if (e?.code === "PIN_REQUIRED") pinAgain = true;
        else if (e?.code === "STALE") refuse("hub.settings.branch.stale");
        else if (e?.code === "TIER_LOCKED") refuse("hub.settings.kitchen.failed_locked");
        else if (e?.code === "VALIDATION_FAILED" && e.field === "ip")
          refuse("hub.settings.printer.invalid.ip");
        else if (e?.code === "PERMISSION_DENIED") refuse("hub.refusal.permission");
        else refuse("hub.settings.failed");
        break;
      }
      if (!status && !pinAgain) status = { tone: "ok", text: t("hub.settings.saved") };
      await load(true, !!status || pinAgain);
    } finally {
      busy = false;
    }
    if (pinAgain) {
      approvalPrompt = "ui.pin.again";
      approving = true;
    }
  }

  async function approve(pin: string) {
    const outcome = await elevate(
      changes.map(([key]) => ({ intent_kind: "settings.set", key })),
      pin,
    );
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    approving = false;
    void save(outcome.ticket);
    return { ok: true } as const;
  }

  async function test() {
    busy = true;
    status = null;
    try {
      const r = await testKotPrint();
      if (r.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const s = r.kind === "ok" ? r.body.status : null;
      if (s?.state === "ok") status = { tone: "ok", text: t("hub.settings.printer.result.ok") };
      else if (s)
        status = {
          tone: "refusal",
          text: t("hub.settings.printer.result.failed", {
            error: t(ERROR_KINDS[s.error_kind ?? "other"]),
          }),
        };
      else if (r.kind === "error" && r.error.code === "TIER_LOCKED") {
        refuse("hub.settings.kitchen.failed_locked");
        await load(true, true);
      } else refuse("hub.settings.printer.test_failed");
    } finally {
      busy = false;
    }
  }

  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      if (!busy && branchLocale.changedKeys.some((k) => KITCHEN_KEYS.includes(k))) void load(true);
    });
  });

  onMount(() => {
    void load();
  });
</script>

<section class="kitchen" aria-labelledby="settings-kitchen-title">
  <h2 class="type-hub-title" id="settings-kitchen-title">{t("hub.settings.kitchen.title")}</h2>
  {#if loadFailed}
    <div class="actions" data-testid="kitchen-load-failed">
      <p class="status refusal type-hub-label" role="status">
        <span aria-hidden="true">⚠</span>
        <span>{t("hub.settings.load_failed")}</span>
      </p>
      <Button
        label="common.retry"
        variant="secondary"
        disabled={loading}
        disabledReason="hub.settings.loading"
        onclick={() => void load()}
      />
    </div>
  {:else if !loading && !unlocked}
    <div class="tier-lock" data-testid="kitchen-tier-lock">
      <p class="lock type-hub-label">
        <span aria-hidden="true">🔒</span>
        {t("hub.settings.kitchen.locked")}
      </p>
      <p class="type-hub-label">{t("hub.settings.kitchen.locked_note")}</p>
    </div>
  {:else if unlocked}
    <p class="type-hub-body muted">{t("hub.settings.kitchen.intro")}</p>
    <form
      class="form"
      data-testid="settings-kitchen"
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <fieldset class="group">
        <legend class="type-hub-label label">{t("hub.settings.kitchen.kot")}</legend>
        {#each KOT_MODES as m (m)}
          <label class="option type-hub-body">
            <input type="radio" name="kot-mode" value={m} bind:group={mode} disabled={busy} />
            <span>{t(MODE_LABELS[m])}</span>
          </label>
        {/each}
      </fieldset>

      {#if mode === "another"}
        <p class="type-hub-body" data-testid="kitchen-selected">
          {chosen
            ? t("hub.settings.printer.selected", { name: chosen.name })
            : t("hub.settings.printer.none_selected")}
        </p>
        <div class="actions">
          <Button
            label="hub.settings.printer.find"
            variant="secondary"
            disabled={busy}
            disabledReason="hub.settings.saving"
            onclick={() => void find()}
          />
        </div>
        {#if askingBluetooth}
          <p class="type-hub-label" data-testid="kitchen-bluetooth-reason">
            {t("hub.settings.printer.bluetooth.reason")}
          </p>
        {/if}
        {#if bluetooth && bluetooth !== "granted"}
          <div class="bluetooth-note" data-testid="kitchen-bluetooth-note">
            <p class="type-hub-label status refusal">
              <span aria-hidden="true">⚠</span>
              <span>{t(BLUETOOTH_NOTES[bluetooth])}</span>
            </p>
            {#if bluetooth === "denied"}
              <Button
                label="hub.settings.printer.bluetooth.open_settings"
                variant="secondary"
                onclick={openAppSettings}
              />
            {/if}
          </div>
        {:else if bluetooth === "granted" && found && !found.some((p) => p.kind === "bluetooth")}
          <p class="type-hub-label" data-testid="kitchen-bluetooth-none">
            {t("hub.settings.printer.bluetooth.none_paired")}
          </p>
        {/if}
        {#if found && !usbSupported}
          <p class="type-hub-label" data-testid="kitchen-usb-unsupported">
            {t("hub.settings.printer.usb_unsupported")}
          </p>
        {/if}
        {#if found}
          <fieldset class="group" data-testid="kitchen-found">
            <legend class="type-hub-label label">{t("hub.settings.printer.found")}</legend>
            {#if found.length === 0}
              <p class="type-hub-label">{t("hub.settings.printer.found_none")}</p>
            {/if}
            {#each found as p (p.id)}
              <label class="option type-hub-body">
                <input
                  type="radio"
                  name="kot-printer"
                  checked={sameSelection(selectionOf(p), chosen)}
                  onchange={() => (chosen = selectionOf(p))}
                />
                <span>{p.name}</span>
              </label>
            {/each}
          </fieldset>
        {/if}
        <div class="field">
          <label class="type-hub-label label" for="kot-ip">{t("hub.settings.printer.ip")}</label>
          <div class="row">
            <input
              id="kot-ip"
              class="control ui-interactive"
              type="text"
              inputmode="decimal"
              autocomplete="off"
              spellcheck="false"
              bind:value={ip}
              onkeydown={(e) => {
                if (e.key !== "Enter") return;
                e.preventDefault();
                addIp();
              }}
              disabled={busy}
            />
            <Button
              label="hub.settings.printer.add"
              variant="secondary"
              disabled={busy}
              disabledReason="hub.settings.saving"
              onclick={addIp}
            />
          </div>
          <p class="type-hub-label muted">{t("hub.settings.printer.ip_hint")}</p>
        </div>
        <fieldset class="group">
          <legend class="type-hub-label label">{t("hub.settings.printer.paper")}</legend>
          {#each PAPER_WIDTHS as w (w)}
            <label class="option type-hub-body">
              <input type="radio" name="kot-paper" value={w} bind:group={paper} disabled={busy} />
              <span>{t(PAPER_LABELS[w])}</span>
            </label>
          {/each}
        </fieldset>
        <div class="actions">
          <Button
            label="hub.settings.printer.test"
            variant="secondary"
            disabled={busy || !saved || changes.length > 0}
            disabledReason={saved
              ? "hub.settings.kitchen.test_saved"
              : "hub.settings.printer.choose_first"}
            onclick={() => void test()}
          />
        </div>
      {/if}

      <div class="group">
        <h3 class="type-hub-label label">{t("hub.settings.kitchen.displays")}</h3>
        {#if displays.length === 0}
          <p class="type-hub-body" data-testid="kitchen-displays">
            {t("hub.settings.kitchen.displays_none")}
          </p>
        {:else}
          <ul class="displays" data-testid="kitchen-displays">
            {#each displays as display (display.id)}
              <li class="display" data-testid="kitchen-display-{display.id}">
                <span class="type-hub-body name">{display.name}</span>
                <span class="type-hub-label state" data-testid="kitchen-display-state">
                  <span aria-hidden="true">{display.connected ? "●" : "○"}</span>
                  {t(
                    display.connected
                      ? "hub.settings.kitchen.display_connected"
                      : "hub.settings.kitchen.display_disconnected",
                  )}
                </span>
                <span class="type-hub-label muted">{lastSeen(display)}</span>
                <Button
                  label="hub.settings.kitchen.revoke"
                  variant="secondary"
                  disabled={!canPair || revoking !== null}
                  disabledReason={canPair
                    ? "hub.settings.saving"
                    : "hub.settings.kitchen.pair_only_managers"}
                  onclick={() => void revoke(display)}
                />
              </li>
            {/each}
          </ul>
        {/if}
        <p
          class="status type-hub-label"
          class:refusal={revokeStatus?.tone === "refusal"}
          role="status"
          data-testid="kitchen-revoke-status"
        >
          {#if revokeStatus}
            {#if revokeStatus.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{revokeStatus.text}</span>
          {/if}
        </p>
        <div class="pair" bind:this={pairButton}>
          {#if !pairOpen}
            <Button
              label="hub.settings.kitchen.pair"
              variant="secondary"
              disabled={!canPair}
              disabledReason="hub.settings.kitchen.pair_only_managers"
              onclick={() => (pairOpen = true)}
            />
          {:else}
            <PairKitchenPanel onclose={() => void closePair()} />
          {/if}
        </div>
      </div>

      <div class="actions">
        <span bind:this={saveWrap}>
          <Button
            label="hub.settings.save"
            type="submit"
            disabled={busy || approving || !changes.length || (mode === "another" && !chosen)}
            disabledReason={approving
              ? "hub.settings.branch.needs_approval"
              : busy
                ? "hub.settings.saving"
                : changes.length
                  ? "hub.settings.printer.choose_first"
                  : "hub.settings.branch.no_changes"}
          />
        </span>
        {#if needsApproval && !approving}
          <p class="type-hub-label" data-testid="kitchen-needs-approval">
            {t("hub.settings.branch.needs_approval")}
          </p>
        {/if}
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="kitchen-status"
        >
          {#if status}
            {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{status.text}</span>
          {/if}
        </p>
      </div>
      {#if approving}
        <InlinePin
          message={approvalPrompt}
          onsubmit={approve}
          oncancel={() => (approving = false)}
          returnFocusTo={saveButton}
        />
      {/if}
    </form>
  {/if}
</section>

<style>
  .kitchen {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  h3,
  p {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .tier-lock {
    display: grid;
    gap: var(--space-1);
    color: var(--tier-lock-foreground);
  }

  .lock {
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .form {
    display: grid;
    gap: var(--space-4);
    max-width: 40rem;
  }

  .field,
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }

  .label {
    padding: 0;
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .option {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
    color: var(--text);
    cursor: pointer;
  }

  .option input {
    flex: none;
    width: var(--space-5);
    height: var(--space-5);
    margin: 0;
    accent-color: var(--accent);
  }

  .row,
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .control {
    --ui-bg: var(--text-field-background);
    flex: 1;
    min-width: 12rem;
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
    font-size: var(--type-web-body-size);
  }

  .control:focus-visible {
    border-width: 2px;
    border-color: var(--accent);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .displays {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .display {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--space-touch-min);
  }

  .display .name {
    flex: 1;
    min-width: 8rem;
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .state {
    display: inline-flex;
    gap: var(--space-2);
    color: var(--text);
  }

  .pair {
    justify-self: stretch;
  }

  .bluetooth-note {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
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
</style>
