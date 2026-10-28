<script lang="ts">
  import { Button, TextField, t } from "@arena/ui";
  import type { PrinterSelected } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import {
    SESSION_DEPENDENCY,
    loadSettings,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { branchLocale } from "../../../lib/locale.svelte";
  import {
    KEY_PRINTER_PAPER_MM,
    KEY_PRINTER_SELECTED,
    PAPER_WIDTHS,
    PRINTER_ERROR_MESSAGES,
    parseSelected,
    testPrint,
  } from "../../../lib/printer";
  import {
    ADDRESS_MAX_LINES,
    FOOTER_MAX_LINES,
    KEY_ADDRESS,
    KEY_BRANCH_NAME,
    KEY_DEFAULT,
    KEY_DRAWER,
    KEY_FOOTER,
    KEY_PHONE,
    LINE_MAX_CHARS,
    RECEIPT_CHOICES,
    lineWidth,
    previewReceipt,
  } from "../../../lib/receipt";
  import { KEY_TAX, parseTax } from "../../../lib/tax";

  type Row = { value: string; rev: number };

  /** The keys this page edits, in save order. */
  const EDITED = [KEY_FOOTER, KEY_ADDRESS, KEY_PHONE, KEY_DEFAULT, KEY_PRINTER_PAPER_MM];
  /** Every key the page reads: the edited ones are required, the rest only enrich it. */
  const READ = [...EDITED, KEY_DRAWER, KEY_PRINTER_SELECTED, KEY_BRANCH_NAME, KEY_TAX];
  /** Saved with the edited keys, but a Hub without the row still opens the page. */
  const SAVED = [...EDITED, KEY_DRAWER];
  const PAPER_LABELS: Record<(typeof PAPER_WIDTHS)[number], MessageKey> = {
    "58": "hub.settings.printer.paper_58",
    "80": "hub.settings.printer.paper_80",
  };
  const CHOICE_LABELS: Record<(typeof RECEIPT_CHOICES)[number], MessageKey> = {
    print: "hub.settings.receipt.default_print",
    none: "hub.settings.receipt.default_none",
  };

  let rows: Record<string, Row> = $state({});
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let testing = $state(false);
  let footer = $state("");
  let address = $state("");
  let phone = $state("");
  let choice = $state("print");
  let paper = $state("80");
  let drawer = $state(false);
  /** The outcome of the last action, shown next to the controls. */
  let status: { tone: "ok" | "refusal"; text: string } | null = $state(null);
  /** After a STALE save: the value now on the Hub (the typed text is kept). */
  let serverValue: string | null = $state(null);
  /** The pending save's idempotency key per key: kept while the same edit is retried. */
  let pendingKeys: Record<string, { value: string; key: string }> = {};

  const form = $derived<Record<string, string>>({
    [KEY_FOOTER]: footer,
    [KEY_ADDRESS]: address,
    [KEY_PHONE]: phone,
    [KEY_DEFAULT]: choice,
    [KEY_PRINTER_PAPER_MM]: paper,
    [KEY_DRAWER]: drawer ? "true" : "false",
  });
  const changes = $derived(SAVED.filter((k) => rows[k] && form[k] !== rows[k].value));
  const tooLong = (text: string) => text.split("\n").some((l) => l.length > LINE_MAX_CHARS);
  /** The first problem with a text block, as the message to show under it. */
  const problem = (text: string, max: number): string | null =>
    text.split("\n").length > max
      ? t("hub.settings.receipt.lines_over", { max })
      : tooLong(text)
        ? t("hub.settings.receipt.line_too_long", { max: LINE_MAX_CHARS })
        : null;
  const footerProblem = $derived(problem(footer, FOOTER_MAX_LINES));
  const addressProblem = $derived(problem(address, ADDRESS_MAX_LINES));
  const phoneProblem = $derived(problem(phone, 1));
  const invalid = $derived(!!(footerProblem || addressProblem || phoneProblem));
  const tax = $derived(parseTax(rows[KEY_TAX]?.value ?? ""));
  const printer: PrinterSelected | null = $derived(
    parseSelected(rows[KEY_PRINTER_SELECTED]?.value),
  );
  const branchName = $derived(rows[KEY_BRANCH_NAME]?.value ?? "");
  const preview = $derived(
    previewReceipt({
      paperMm: paper,
      name: branchName,
      address,
      phone,
      gstin: tax?.gst ? tax.gstin : "",
      footer,
      body: t("hub.settings.receipt.preview_body"),
    }),
  );

  async function load(keepEdits = false): Promise<boolean> {
    loading = true;
    try {
      const result = await loadSettings();
      if (result.kind === "signed_out") {
        await invalidate(SESSION_DEPENDENCY);
        return false;
      }
      const next: Record<string, Row> = {};
      if (result.kind === "ok") {
        for (const s of result.settings) {
          if (READ.includes(s.key)) next[s.key] = { value: s.value, rev: s.rev };
        }
      }
      loadFailed = EDITED.some((k) => next[k] === undefined);
      if (loadFailed) return false;
      rows = next;
      if (!keepEdits) {
        footer = next[KEY_FOOTER].value;
        address = next[KEY_ADDRESS].value;
        phone = next[KEY_PHONE].value;
        choice = next[KEY_DEFAULT].value;
        paper = next[KEY_PRINTER_PAPER_MM].value;
        drawer = next[KEY_DRAWER]?.value === "true";
      }
      return true;
    } finally {
      loading = false;
    }
  }

  async function save() {
    if (saving || invalid || !changes.length) return;
    saving = true;
    status = null;
    serverValue = null;
    let refused = false;
    let reloaded = false;
    try {
      for (const key of changes) {
        const value = form[key];
        // One key per edit: a retry of the same text reuses it.
        if (pendingKeys[key]?.value !== value) {
          pendingKeys[key] = { value, key: newIdempotencyKey() };
        }
        const outcome = await submitIntent(
          { kind: "settings.set", key, value },
          pendingKeys[key].key,
          rows[key].rev,
        );
        if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
        if (outcome.kind === "applied") {
          delete pendingKeys[key];
          continue;
        }
        refused = true;
        const code = outcome.kind === "error" ? outcome.error.code : null;
        if (code === "PERMISSION_DENIED") {
          // Refused inline, next to the control; never a modal and never naming anyone.
          delete pendingKeys[key];
          status = { tone: "refusal", text: t("hub.refusal.permission") };
        } else if (code === "STALE") {
          delete pendingKeys[key];
          status = { tone: "refusal", text: t("hub.settings.stale") };
          await load(true);
          reloaded = true;
          serverValue = rows[key]?.value ?? null;
        } else status = { tone: "refusal", text: t("hub.settings.failed") };
        break;
      }
      // The Hub's revs (and, once all saved, its values), not a guess.
      if (!reloaded) await load(refused);
      if (!refused) status = { tone: "ok", text: t("hub.settings.saved") };
    } finally {
      saving = false;
    }
  }

  async function test() {
    testing = true;
    status = null;
    try {
      const r = await testPrint();
      if (r.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const s = r.kind === "ok" ? r.body.status : null;
      if (s?.state === "ok") status = { tone: "ok", text: t("hub.settings.printer.result.ok") };
      else if (s) {
        status = {
          tone: "refusal",
          text: t("hub.settings.printer.result.failed", {
            error: t(PRINTER_ERROR_MESSAGES[s.error_kind ?? "other"]),
          }),
        };
      } else {
        status = { tone: "refusal", text: t("hub.settings.printer.test_failed") };
      }
    } finally {
      testing = false;
    }
  }

  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      if (!saving && branchLocale.changedKeys.some((k) => READ.includes(k))) {
        void load(changes.length > 0);
      }
    });
  });

  onMount(() => {
    void load();
  });
</script>

<section class="receipt" aria-labelledby="settings-receipt-title">
  <h2 class="type-hub-title" id="settings-receipt-title">{t("hub.settings.receipt.title")}</h2>
  <p class="type-hub-body muted">{t("hub.settings.receipt.intro")}</p>
  <form
    class="form"
    data-testid="settings-receipt-footer"
    novalidate
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    {#if loadFailed}
      <div class="actions" data-testid="settings-load-failed">
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
    {:else if rows[KEY_FOOTER] !== undefined}
      <fieldset class="group" data-testid="receipt-printer">
        <legend class="type-hub-label label">{t("hub.settings.receipt.printer")}</legend>
        <p class="type-hub-body" data-testid="receipt-printer-name">
          {printer
            ? t("hub.settings.printer.selected", { name: printer.name })
            : t("hub.settings.printer.none_selected")}
        </p>
        <div class="actions">
          <a class="link ui-interactive type-hub-label" href="/settings/printer"
            >{t("hub.settings.receipt.printer_find")}</a
          >
          <Button
            label="hub.settings.printer.test"
            variant="secondary"
            disabled={testing ||
              saving ||
              loading ||
              !printer ||
              paper !== rows[KEY_PRINTER_PAPER_MM]?.value}
            disabledReason={testing
              ? "hub.settings.printer.testing"
              : paper !== rows[KEY_PRINTER_PAPER_MM]?.value
                ? "hub.settings.receipt.test_saved"
                : "hub.settings.printer.none_selected"}
            onclick={() => void test()}
          />
        </div>
        <div class="options" role="radiogroup" aria-label={t("hub.settings.printer.paper")}>
          {#each PAPER_WIDTHS as w (w)}
            <label class="option type-hub-body">
              <input type="radio" name="paper" value={w} bind:group={paper} disabled={saving} />
              <span>{t(PAPER_LABELS[w])}</span>
            </label>
          {/each}
        </div>
      </fieldset>

      <fieldset class="group">
        <legend class="type-hub-label label">{t("hub.settings.receipt.header")}</legend>
        <p class="type-hub-body" data-testid="receipt-branch-name">
          {t("hub.settings.receipt.header_name", { name: branchName })}
          <a class="inline-link" href="/settings/branch"
            >{t("hub.settings.receipt.header_name_change")}</a
          >
        </p>
        <div class="field">
          <label class="label type-hub-label" for="receipt-address"
            >{t("hub.settings.receipt.address")}</label
          >
          <textarea
            class="control ui-interactive"
            id="receipt-address"
            rows="3"
            bind:value={address}
            aria-invalid={addressProblem ? "true" : undefined}
            aria-describedby="receipt-address-note"></textarea>
          <p class="note type-hub-label" id="receipt-address-note">
            {#if addressProblem}<span aria-hidden="true">⚠</span>
              {addressProblem}{:else}{t("hub.settings.receipt.footer_hint")}{/if}
          </p>
        </div>
        <TextField
          label="hub.settings.receipt.phone"
          name="receipt_phone"
          type="tel"
          bind:value={phone}
        />
        {#if phoneProblem}
          <p class="note type-hub-label">
            <span aria-hidden="true">⚠</span>
            {phoneProblem}
          </p>
        {/if}
        {#if tax?.gst}
          <p class="type-hub-label muted">{t("hub.settings.receipt.gstin_auto")}</p>
        {/if}
      </fieldset>

      <div class="field">
        <label class="label type-hub-label" for="receipt-footer"
          >{t("hub.settings.receipt_footer")}</label
        >
        <textarea
          class="control ui-interactive"
          id="receipt-footer"
          name="receipt_footer"
          rows="3"
          bind:value={footer}
          aria-invalid={footerProblem ? "true" : undefined}
          aria-describedby="receipt-footer-note"></textarea>
        <p class="note type-hub-label" id="receipt-footer-note">
          {#if footerProblem}<span aria-hidden="true">⚠</span>
            {footerProblem}{:else}{t("hub.settings.receipt.footer_hint")}{/if}
        </p>
      </div>

      <fieldset class="group">
        <legend class="type-hub-label label">{t("hub.settings.receipt.default")}</legend>
        <div class="options" role="radiogroup" aria-label={t("hub.settings.receipt.default")}>
          {#each RECEIPT_CHOICES as c (c)}
            <label class="option type-hub-body">
              <input
                type="radio"
                name="receipt-default"
                value={c}
                bind:group={choice}
                disabled={saving}
              />
              <span>{t(CHOICE_LABELS[c])}</span>
            </label>
          {/each}
        </div>
      </fieldset>

      <div class="group" data-testid="receipt-drawer">
        <label class="option type-hub-body">
          <input
            type="checkbox"
            name="receipt-drawer"
            bind:checked={drawer}
            disabled={saving || rows[KEY_DRAWER] === undefined}
            aria-describedby="receipt-drawer-note"
          />
          <span>{t("hub.settings.receipt.drawer")}</span>
        </label>
        <p class="note type-hub-label" id="receipt-drawer-note">
          {t("hub.settings.receipt.drawer_hint")}
        </p>
      </div>

      <div class="group" data-testid="receipt-preview-box">
        <p class="type-hub-label label">
          {t("hub.settings.receipt.preview")}
          <span class="muted"
            >· {t("hub.settings.receipt.preview_width", { chars: lineWidth(paper) })}</span
          >
        </p>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrollable region must take focus) -->
        <pre
          class="preview"
          role="region"
          tabindex="0"
          data-testid="receipt-preview"
          aria-label={t("hub.settings.receipt.preview")}>{preview.join("\n")}</pre>
      </div>

      <div class="actions">
        <Button
          label="hub.settings.save"
          type="submit"
          disabled={saving || loading || invalid || !changes.length}
          disabledReason={saving
            ? "hub.settings.saving"
            : loading
              ? "hub.settings.loading"
              : "hub.settings.receipt.footer_hint"}
        />
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="settings-status"
        >
          {#if status}
            {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{status.text}</span>
          {/if}
        </p>
      </div>
      {#if serverValue !== null}
        <p class="type-hub-label server" data-testid="settings-server-value">
          {t("hub.settings.server_value", { value: serverValue })}
        </p>
      {/if}
    {/if}
  </form>
</section>

<style>
  .receipt {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  p {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
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

  .options,
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
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

  .link {
    --ui-bg: var(--button-secondary-background);
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--button-secondary-foreground);
    font-weight: var(--type-hub-title-weight);
    text-decoration: none;
  }

  .inline-link {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    color: var(--text);
    text-decoration: underline;
  }

  .control {
    --ui-bg: var(--text-field-background);
    min-width: 0;
    box-sizing: border-box;
    padding: var(--space-2) var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
    font-size: var(--type-web-body-size);
    resize: vertical;
  }

  .control:focus-visible {
    border-width: 2px;
    border-color: var(--accent);
    padding: calc(var(--space-2) - 1px) calc(var(--space-3) - 1px);
  }

  .control[aria-invalid="true"] {
    border-width: 2px;
    border-color: var(--danger);
  }

  .note {
    color: var(--text-muted);
  }

  .preview {
    margin: 0;
    padding: var(--space-3);
    overflow-x: auto;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: var(--type-hub-label-size);
    line-height: 1.4;
    white-space: pre;
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
