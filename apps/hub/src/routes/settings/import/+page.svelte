<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { ProductImportPreview, ProductImportResult } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { SESSION_DEPENDENCY, hasPermission, newIdempotencyKey } from "../../../lib/auth";
  import {
    MAX_IMPORT_BYTES,
    commitProducts,
    previewProducts,
    saveProductTemplate,
  } from "../../../lib/import";
  import ScrollRegion from "../../../lib/ScrollRegion.svelte";
  import { sessionStaff } from "../../../lib/operating";
  import { folderProblem } from "../../../lib/reports";

  const staff = $derived(sessionStaff(page.data));
  const allowed = $derived(hasPermission(staff, "catalog.edit"));

  let folder = $state("");
  let folderError: MessageKey | null = $state(null);
  let templateBusy = $state(false);
  let templateStatus: { ok: boolean; text: string } | null = $state(null);

  /** The file chosen: its text, and the id of this attempt (kept so a retry never duplicates). */
  let csv = $state("");
  let importId = "";
  let checking = $state(false);
  let importing = $state(false);
  let preview = $state<ProductImportPreview | null>(null);
  let result = $state<ProductImportResult | null>(null);
  let problem: string | null = $state(null);

  const busy = $derived(checking || importing);
  const rowsToImport = $derived(preview ? preview.new + preview.updated : 0);

  async function saveTemplate() {
    folderError = null;
    templateStatus = null;
    const target = folder.trim();
    if (!target) {
      folderError = folderProblem("");
      return;
    }
    templateBusy = true;
    const outcome = await saveProductTemplate(target);
    templateBusy = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "ok") {
      templateStatus = {
        ok: true,
        text: t("hub.import.template_saved", { path: outcome.view.path }),
      };
    } else if (outcome.kind === "invalid" && outcome.field === "folder") {
      folderError = folderProblem(outcome.message);
    } else {
      templateStatus = { ok: false, text: t("hub.import.template_failed") };
    }
  }

  async function choose(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    preview = result = null;
    problem = null;
    csv = "";
    if (!file) return;
    if (file.size > MAX_IMPORT_BYTES) {
      problem = t("hub.import.too_big");
      return;
    }
    checking = true;
    try {
      csv = await file.text();
    } catch {
      checking = false;
      problem = t("hub.import.unreadable");
      return;
    }
    importId = newIdempotencyKey();
    const outcome = await previewProducts(csv);
    checking = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "ok") preview = outcome.view;
    else if (outcome.kind === "invalid")
      problem = t("hub.import.refused", { message: outcome.message });
    else problem = t(outcome.kind === "denied" ? "hub.import.no_access" : "hub.import.unreachable");
  }

  async function run(validOnly: boolean) {
    if (!preview || busy) return;
    problem = null;
    importing = true;
    const outcome = await commitProducts(csv, importId, validOnly);
    importing = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "ok") {
      result = outcome.view;
      preview = null;
      csv = "";
    } else if (outcome.kind === "invalid") {
      problem = t("hub.import.refused", { message: outcome.message });
    } else {
      problem = t(outcome.kind === "denied" ? "hub.import.no_access" : "hub.import.failed");
    }
  }
</script>

<section class="import" aria-labelledby="settings-import-title" data-testid="settings-import">
  <h2 class="type-hub-title" id="settings-import-title">{t("hub.import.title")}</h2>
  <h3 class="type-hub-body heading">{t("hub.import.products_title")}</h3>
  {#if !allowed}
    <p class="type-hub-body muted" data-testid="import-no-access">{t("hub.import.no_access")}</p>
  {:else}
    <p class="type-hub-body muted">{t("hub.import.products_intro")}</p>

    <div class="row">
      <label class="field grow type-hub-label">
        <span>{t("hub.import.template_folder")}</span>
        <input
          class="ui-interactive"
          class:invalid={!!folderError}
          type="text"
          autocomplete="off"
          spellcheck="false"
          bind:value={folder}
          disabled={templateBusy}
          aria-invalid={folderError ? "true" : undefined}
          aria-describedby="template-status"
          data-testid="template-folder"
        />
      </label>
      <Button
        label="hub.import.template_button"
        variant="secondary"
        disabled={templateBusy}
        disabledReason="hub.import.template_saving"
        onclick={() => void saveTemplate()}
      />
    </div>
    <div role="status" id="template-status">
      {#if folderError}
        <p class="type-hub-label warn" data-testid="template-folder-error">
          <span aria-hidden="true">⚠</span>
          {t(folderError)}
        </p>
      {:else if templateStatus}
        <p
          class="type-hub-label"
          class:warn={!templateStatus.ok}
          data-testid="template-status-text"
        >
          {templateStatus.text}
        </p>
      {/if}
    </div>

    <label class="field type-hub-label">
      <span>{t("hub.import.file")}</span>
      <input
        class="ui-interactive file"
        type="file"
        accept=".csv,text/csv"
        disabled={busy}
        aria-describedby="import-hint"
        onchange={choose}
        data-testid="import-file"
      />
    </label>
    <p class="type-hub-label muted" id="import-hint">{t("hub.import.file_hint")}</p>

    <div role="status" class="outcome">
      {#if checking}
        <p class="type-hub-label muted">{t("hub.import.checking")}</p>
      {:else if importing}
        <p class="type-hub-label muted">{t("hub.import.importing")}</p>
      {/if}
      {#if problem}
        <p class="type-hub-label warn" data-testid="import-problem">
          <span aria-hidden="true">⚠</span>
          {problem}
        </p>
      {/if}
      {#if result}
        <p class="type-hub-body" data-testid="import-done">
          {t("hub.import.done", { created: result.created, updated: result.updated })}
          {#if result.skipped > 0}
            {t("hub.import.done_skipped", { skipped: result.skipped })}
          {/if}
        </p>
      {/if}
    </div>

    {#if preview}
      <div class="preview" data-testid="import-preview">
        <p class="type-hub-body" data-testid="import-summary">
          {t("hub.import.summary", {
            rows: preview.rows,
            new: preview.new,
            updated: preview.updated,
            failed: preview.failed,
          })}
        </p>
        {#if preview.with_stock > 0}
          <p class="type-hub-label muted">
            {t("hub.import.stock_note", { count: preview.with_stock })}
          </p>
        {/if}
        {#if preview.unusable}
          <p class="type-hub-label warn" data-testid="import-unusable">
            <span aria-hidden="true">⚠</span>
            {t("hub.import.unusable")}
          </p>
        {/if}
        {#if preview.failures.length > 0}
          <h4 class="type-hub-label">{t("hub.import.problems")}</h4>
          <ScrollRegion label="hub.import.problems">
            <table data-testid="import-failures">
              <thead>
                <tr class="type-hub-label">
                  <th scope="col">{t("hub.import.col_row")}</th>
                  <th scope="col">{t("hub.import.col_column")}</th>
                  <th scope="col">{t("hub.import.col_problem")}</th>
                </tr>
              </thead>
              <tbody class="type-hub-body">
                {#each preview.failures as f, i (i)}
                  <tr data-testid="import-failure">
                    <td class="num">{f.row > 0 ? f.row : t("hub.import.row_file")}</td>
                    <td>{f.field}</td>
                    <td>{f.message}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </ScrollRegion>
          {#if preview.truncated}
            <p class="type-hub-label muted">
              {t("hub.import.truncated", { count: preview.failures.length })}
            </p>
          {/if}
        {/if}
        <div class="row">
          <Button
            label="hub.import.commit"
            disabled={busy || !preview.can_commit}
            disabledReason={busy
              ? "hub.import.importing"
              : rowsToImport === 0
                ? "hub.import.commit_nothing"
                : "hub.import.commit_blocked"}
            onclick={() => void run(false)}
          />
          {#if preview.failed > 0 || preview.failures.length > 0}
            <Button
              label="hub.import.commit_valid"
              variant="secondary"
              disabled={busy || !preview.can_commit_valid}
              disabledReason={busy ? "hub.import.importing" : "hub.import.commit_nothing"}
              onclick={() => void run(true)}
            />
          {/if}
        </div>
      </div>
    {/if}
  {/if}
</section>

<style>
  .import {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  h3,
  h4,
  p {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .warn {
    color: var(--text-field-error-text);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: var(--space-3);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  .grow {
    flex: 1 1 20rem;
    max-inline-size: 36rem;
  }

  input[type="text"] {
    --ui-bg: var(--text-field-background);
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

  input.invalid {
    border-color: var(--text-field-error-text);
  }

  .file {
    max-inline-size: 36rem;
  }

  .outcome {
    display: grid;
    gap: var(--space-2);
  }

  .preview {
    display: grid;
    gap: var(--space-3);
  }

  table {
    border-collapse: collapse;
    inline-size: 100%;
  }

  th,
  td {
    padding: var(--space-1) var(--space-2);
    border-block-end: 1px solid var(--border-strong);
    text-align: start;
    vertical-align: top;
  }

  .num {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
