<script lang="ts">
  import { Button, InlinePin, t } from "@arena/ui";
  import type { FoundPrinter, PrinterErrorKind, PrinterSelected } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, untrack } from "svelte";
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
  let ip = $state("");
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; text: string } | null = $state(null);
  let saveWrap: HTMLElement | undefined = $state();
  const saveButton = $derived(saveWrap?.querySelector("button") ?? null);

  const staff = $derived(sessionStaff(page.data));
  const needsApproval = $derived(!hasPermission(staff, "settings.edit"));
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

  async function find() {
    busy = true;
    status = null;
    try {
      await askBluetooth();
      const r = await findPrinters();
      if (r.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (r.kind === "ok") found = r.body.printers;
      else refuse("hub.settings.printer.find_failed");
    } finally {
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
        <p class="type-hub-body" data-testid="kitchen-displays">
          {t("hub.settings.kitchen.displays_none")}
        </p>
        <div class="actions">
          <Button
            label="hub.settings.kitchen.pair"
            variant="secondary"
            disabled={true}
            disabledReason="hub.settings.kitchen.pair_later"
          />
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
