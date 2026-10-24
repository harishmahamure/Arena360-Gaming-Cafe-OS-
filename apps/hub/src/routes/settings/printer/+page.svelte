<script lang="ts">
  import { Button, formatDateTime, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type {
    FoundPrinter,
    PrinterErrorKind,
    PrinterSelected,
    PrinterStatus,
    PrinterStatusReply,
  } from "@arena/ui/generated";
  import { onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    hasPermission,
    loadSettings,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { branchLocale } from "../../../lib/locale.svelte";
  import { sessionStaff } from "../../../lib/operating";
  import {
    KEY_PRINTER_PAPER_MM,
    KEY_PRINTER_SELECTED,
    KEY_PRINTER_STATUS,
    PAPER_WIDTHS,
    PRINTER_KEYS,
    findPrinters,
    parseSelected,
    parseStatus,
    sameSelection,
    selectionOf,
    skipPrinter,
    testPrint,
  } from "../../../lib/printer";

  type Row = { value: string; rev: number };
  type Notice =
    | { tone: "ok" | "refusal"; message: MessageKey }
    | { tone: "refusal"; message: "hub.settings.printer.result.failed"; error: string };

  const PAPER_LABELS: Record<(typeof PAPER_WIDTHS)[number], MessageKey> = {
    "58": "hub.settings.printer.paper_58",
    "80": "hub.settings.printer.paper_80",
  };
  const ERROR_KINDS: Record<PrinterErrorKind, MessageKey> = {
    refused: "hub.settings.printer.error.refused",
    timeout: "hub.settings.printer.error.timeout",
    unreachable: "hub.settings.printer.error.unreachable",
    spooler: "hub.settings.printer.error.spooler",
    other: "hub.settings.printer.error.other",
  };
  const KIND_LABELS: Record<FoundPrinter["kind"], MessageKey> = {
    usb: "hub.settings.printer.kind.usb",
    lan: "hub.settings.printer.kind.lan",
  };

  let rows: Record<string, Row> = $state({});
  let loading = $state(true);
  let ready = $state(false);
  let loadFailed = $state(false);
  let busy: "find" | "select" | "paper" | "add" | "test" | "skip" | null = $state(null);
  let found: FoundPrinter[] | null = $state(null);
  let findNotice: MessageKey | null = $state(null);
  let ip = $state("");
  let ipError: MessageKey | null = $state(null);
  let ipInput: HTMLInputElement | undefined = $state();
  let notice: Notice | null = $state(null);
  let loadSeq = 0;

  const staff = $derived(sessionStaff(page.data));
  const canEdit = $derived(hasPermission(staff, "settings.edit"));
  const selected = $derived(parseSelected(rows[KEY_PRINTER_SELECTED]?.value));
  const paper = $derived(rows[KEY_PRINTER_PAPER_MM]?.value ?? "80");
  const status = $derived(parseStatus(rows[KEY_PRINTER_STATUS]?.value));
  const locked = $derived(!canEdit || busy !== null || loading);
  const inputsDisabled = $derived(!canEdit || !ready);
  const lockReason: MessageKey = $derived(
    !canEdit
      ? "hub.settings.printer.manager_only"
      : busy === "find"
        ? "hub.settings.printer.finding"
        : busy === "test"
          ? "hub.settings.printer.testing"
          : busy !== null
            ? "hub.settings.saving"
            : "hub.settings.loading",
  );

  async function load(quiet = false) {
    const seq = ++loadSeq;
    if (!quiet) loading = true;
    try {
      const result = await loadSettings();
      if (seq !== loadSeq) return;
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const next: Record<string, Row> = {};
      if (result.kind === "ok") {
        for (const s of result.settings) {
          if (PRINTER_KEYS.includes(s.key)) next[s.key] = { value: s.value, rev: s.rev };
        }
      }
      if (PRINTER_KEYS.some((k) => next[k] === undefined)) {
        loadFailed = true;
        return;
      }
      loadFailed = false;
      rows = next;
      ready = true;
    } finally {
      if (seq === loadSeq && !quiet) loading = false;
    }
  }

  async function save(key: string, value: string): Promise<"applied" | "ip" | "failed"> {
    const row = rows[key];
    if (!row) return "failed";
    const outcome = await submitIntent(
      { kind: "settings.set", key, value },
      newIdempotencyKey(),
      row.rev,
    );
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return "failed";
    }
    if (outcome.kind === "applied") {
      await load(true);
      return "applied";
    }
    const e = outcome.kind === "error" ? outcome.error : null;
    if (e?.code === "VALIDATION_FAILED" && e.field === "ip") return "ip";
    if (e?.code === "STALE") {
      notice = { tone: "refusal", message: "hub.settings.branch.stale" };
      await load(true);
    } else if (e?.code === "PERMISSION_DENIED") {
      notice = { tone: "refusal", message: "hub.settings.printer.manager_only" };
    } else {
      notice = { tone: "refusal", message: "hub.settings.failed" };
    }
    return "failed";
  }

  async function find() {
    if (locked) return;
    busy = "find";
    findNotice = null;
    notice = null;
    try {
      const r = await findPrinters();
      if (r.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (r.kind === "ok") {
        found = r.body.printers;
      } else if (r.kind === "error" && r.error.code === "PERMISSION_DENIED") {
        findNotice = "hub.settings.printer.manager_only";
      } else {
        findNotice = "hub.settings.printer.find_failed";
      }
    } finally {
      busy = null;
    }
  }

  async function choose(p: PrinterSelected) {
    if (locked || sameSelection(p, selected)) return;
    busy = "select";
    notice = null;
    try {
      if ((await save(KEY_PRINTER_SELECTED, JSON.stringify(p))) === "applied") {
        notice = { tone: "ok", message: "hub.settings.saved" };
      }
    } finally {
      busy = null;
    }
  }

  async function add() {
    if (locked) return;
    const text = ip.trim();
    ipError = null;
    notice = null;
    if (!text) {
      ipError = "hub.settings.printer.ip_empty";
    } else {
      busy = "add";
      try {
        const outcome = await save(
          KEY_PRINTER_SELECTED,
          JSON.stringify({ kind: "lan", id_or_addr: text, name: text }),
        );
        if (outcome === "applied") {
          ip = "";
          notice = { tone: "ok", message: "hub.settings.saved" };
        } else if (outcome === "ip") {
          ipError = "hub.settings.printer.invalid.ip";
        }
      } finally {
        busy = null;
      }
    }
    if (ipError) {
      await tick();
      ipInput?.focus();
    }
  }

  async function choosePaper(value: string) {
    if (locked || value === paper) return;
    busy = "paper";
    notice = null;
    try {
      if ((await save(KEY_PRINTER_PAPER_MM, value)) === "applied") {
        notice = { tone: "ok", message: "hub.settings.saved" };
      }
    } finally {
      busy = null;
    }
  }

  function noticeOf(reply: PrinterStatusReply): Notice {
    const s = reply.status;
    if (reply.outcome === "printed_not_recorded" || !s)
      return { tone: "refusal", message: "hub.settings.printer.result.printed_not_recorded" };
    if (s.state === "ok") return { tone: "ok", message: "hub.settings.printer.result.ok" };
    if (s.state === "skipped")
      return { tone: "ok", message: "hub.settings.printer.result.skipped" };
    return { tone: "refusal", message: "hub.settings.printer.result.failed", error: kindText(s) };
  }

  async function run(kind: "test" | "skip") {
    if (locked || (kind === "test" && !selected)) return;
    busy = kind;
    notice = null;
    try {
      const r = kind === "test" ? await testPrint() : await skipPrinter();
      if (r.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (r.kind === "ok") {
        notice = noticeOf(r.body);
        await load(true);
      } else if (r.kind === "error" && r.error.code === "PERMISSION_DENIED") {
        notice = { tone: "refusal", message: "hub.settings.printer.manager_only" };
      } else if (r.kind === "error" && r.error.field === KEY_PRINTER_SELECTED) {
        notice = { tone: "refusal", message: "hub.settings.printer.choose_first" };
      } else {
        notice = { tone: "refusal", message: "hub.settings.printer.test_failed" };
      }
    } finally {
      busy = null;
    }
  }

  function kindText(s: PrinterStatus): string {
    return t(ERROR_KINDS[s.error_kind ?? "other"]);
  }

  function statusText(s: PrinterStatus | null): string {
    if (!s || s.state === "unset" || s.at === null) return t("hub.settings.printer.status.unset");
    const date = formatDateTime(s.at, branchLocale.current?.tz);
    if (s.state === "ok") return t("hub.settings.printer.status.ok", { date });
    if (s.state === "skipped") return t("hub.settings.printer.status.skipped", { date });
    return t("hub.settings.printer.status.failed", { date, error: kindText(s) });
  }

  function optionText(p: FoundPrinter): string {
    const kind = t(KIND_LABELS[p.kind]);
    return p.kind === "lan" && p.addr
      ? t("hub.settings.printer.option_lan", {
          name: p.name,
          kind,
          ip: p.addr.replace(/:9100$/, ""),
        })
      : t("hub.settings.printer.option", { name: p.name, kind });
  }

  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      if (busy === null && branchLocale.changedKeys.some((k) => PRINTER_KEYS.includes(k)))
        void load(true);
    });
  });

  onMount(() => {
    void load();
  });

  const ipDescribedBy = $derived(
    ["printer-ip-hint", ipError ? "printer-ip-error" : null].filter(Boolean).join(" "),
  );
</script>

<div class="printer">
  <section class="block" aria-labelledby="settings-printer-title">
    <h2 class="type-hub-title" id="settings-printer-title">{t("hub.settings.printer.title")}</h2>
    <p class="intro type-hub-body">{t("hub.settings.printer.intro")}</p>

    {#if loadFailed}
      <div class="actions" data-testid="printer-load-failed">
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
    {:else}
      <div class="form" data-testid="settings-printer">
        <p class="current type-hub-body" data-testid="printer-selected">
          {selected
            ? t("hub.settings.printer.selected", { name: selected.name })
            : t("hub.settings.printer.none_selected")}
        </p>
        <p class="note type-hub-label" data-testid="printer-last-status">{statusText(status)}</p>

        <div class="field">
          <div class="actions">
            <Button
              label="hub.settings.printer.find"
              variant="secondary"
              disabled={locked}
              disabledReason={lockReason}
              onclick={() => void find()}
            />
          </div>
          <p class="note type-hub-label" role="status" data-testid="printer-find-status">
            {#if busy === "find"}{t("hub.settings.printer.finding")}{:else if findNotice}<span
                aria-hidden="true">⚠</span
              >
              <span>{t(findNotice)}</span>{:else if found && found.length === 0}{t(
                "hub.settings.printer.found_none",
              )}{/if}
          </p>
          {#if found && found.length > 0}
            <fieldset class="group" data-testid="printer-found">
              <legend class="label type-hub-label">{t("hub.settings.printer.found")}</legend>
              <div class="options column">
                {#each found as p (p.id)}
                  <label class="option type-hub-body">
                    <input
                      type="radio"
                      name="printer"
                      value={p.id}
                      checked={sameSelection(selectionOf(p), selected)}
                      disabled={inputsDisabled}
                      onchange={() => void choose(selectionOf(p))}
                    />
                    <span>{optionText(p)}</span>
                  </label>
                {/each}
              </div>
            </fieldset>
          {/if}
        </div>

        <form
          class="field"
          novalidate
          onsubmit={(e) => {
            e.preventDefault();
            void add();
          }}
        >
          <label class="label type-hub-label" for="printer-ip">{t("hub.settings.printer.ip")}</label
          >
          <div class="row">
            <input
              id="printer-ip"
              class="control ui-interactive"
              class:invalid={!!ipError}
              type="text"
              inputmode="decimal"
              autocomplete="off"
              spellcheck="false"
              bind:this={ipInput}
              bind:value={ip}
              disabled={inputsDisabled}
              aria-invalid={ipError ? "true" : undefined}
              aria-describedby={ipDescribedBy}
            />
            <Button
              label="hub.settings.printer.add"
              type="submit"
              variant="secondary"
              disabled={locked}
              disabledReason={lockReason}
            />
          </div>
          <p class="hint type-hub-label" id="printer-ip-hint">
            {t("hub.settings.printer.ip_hint")}
          </p>
          <p class="error type-hub-label" id="printer-ip-error" aria-live="polite">
            {#if ipError}<span aria-hidden="true">⚠</span>
              <span>{t(ipError)}</span>{/if}
          </p>
        </form>

        <fieldset class="field group">
          <legend class="label type-hub-label">{t("hub.settings.printer.paper")}</legend>
          <div class="options">
            {#each PAPER_WIDTHS as width (width)}
              <label class="option type-hub-body">
                <input
                  type="radio"
                  name="paper"
                  value={width}
                  checked={paper === width}
                  disabled={inputsDisabled}
                  onchange={() => void choosePaper(width)}
                />
                <span>{t(PAPER_LABELS[width])}</span>
              </label>
            {/each}
          </div>
        </fieldset>

        <div class="actions">
          <Button
            label="hub.settings.printer.test"
            disabled={locked || !selected}
            disabledReason={!locked && !selected ? "hub.settings.printer.choose_first" : lockReason}
            onclick={() => void run("test")}
          />
          <Button
            label="hub.settings.printer.skip"
            variant="secondary"
            disabled={locked}
            disabledReason={lockReason}
            onclick={() => void run("skip")}
          />
        </div>
        <p class="hint type-hub-label" data-testid="printer-skip-note">
          {t("hub.settings.printer.skip_note")}
        </p>
        <p
          class="status type-hub-label"
          class:refusal={notice?.tone === "refusal"}
          role="status"
          data-testid="printer-status"
        >
          {#if busy === "test"}
            <span>{t("hub.settings.printer.testing")}</span>
          {:else if notice}
            {#if notice.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span
              >{"error" in notice
                ? t(notice.message, { error: notice.error })
                : t(notice.message)}</span
            >
          {/if}
        </p>
      </div>
    {/if}
  </section>

  <section class="block" aria-labelledby="settings-payments-title">
    <h2 class="type-hub-title" id="settings-payments-title">{t("hub.settings.payments.title")}</h2>
    <p class="type-hub-body payments" data-testid="payments-text">
      {t("hub.settings.payments.text")}
    </p>
  </section>
</div>

<style>
  .printer {
    display: grid;
    gap: var(--space-6);
  }

  .block {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  .intro,
  .payments,
  .current {
    margin: 0;
  }

  .intro {
    color: var(--text-muted);
  }

  .payments,
  .current {
    color: var(--text);
    max-width: 40rem;
  }

  .form {
    display: grid;
    gap: var(--space-4);
    max-width: 40rem;
  }

  .field {
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

  .options {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-5);
  }

  .options.column {
    flex-direction: column;
    gap: var(--space-1);
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

  .option:has(input:disabled) {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .row {
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

  .control.invalid {
    border-width: 2px;
    border-color: var(--danger);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .control:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .hint,
  .note,
  .error {
    margin: 0;
  }

  .hint {
    color: var(--text-muted);
  }

  .note {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
    color: var(--text);
  }

  .error {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
    color: var(--text-field-error-text);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .status {
    display: flex;
    gap: var(--space-2);
    margin: 0;
    min-height: 1lh;
    color: var(--text);
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }
</style>
