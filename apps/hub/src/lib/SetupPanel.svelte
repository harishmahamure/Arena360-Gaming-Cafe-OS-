<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { SetupPreview, SetupFileResult } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, newIdempotencyKey } from "./auth";
  import {
    MAX_SETUP_BYTES,
    commitSetup,
    exportSetup,
    previewSetup,
    type ImportOutcome,
  } from "./import";
  import ScrollRegion from "./ScrollRegion.svelte";
  import { folderProblem } from "./reports";

  let { allowed }: { allowed: boolean } = $props();

  const KIND: Record<string, MessageKey> = {
    group: "hub.import.setup_kind_group",
    rate: "hub.import.setup_kind_rate",
    product: "hub.import.setup_kind_product",
    bundle: "hub.import.setup_kind_bundle",
    plan: "hub.import.setup_kind_plan",
    rule: "hub.import.setup_kind_rule",
    coupon: "hub.import.setup_kind_coupon",
  };
  const SKIP: Record<string, MessageKey> = {
    exists: "hub.import.starter_skip_exists",
    invalid: "hub.import.starter_skip_invalid",
    no_group: "hub.import.setup_skip_no_group",
    no_product: "hub.import.setup_skip_no_product",
    no_plan: "hub.import.setup_skip_no_plan",
  };

  let folder = $state("");
  let folderError: MessageKey | null = $state(null);
  let exporting = $state(false);
  let exported: { ok: boolean; text: string } | null = $state(null);

  /** The file chosen: its text, and the id of this attempt (kept so a retry never duplicates). */
  let json = "";
  let importId = "";
  let checking = $state(false);
  let adding = $state(false);
  let preview: SetupPreview | null = $state(null);
  let result: SetupFileResult | null = $state(null);
  let problem: string | null = $state(null);

  const busy = $derived(checking || adding);

  /** Sorts out the shared outcomes; the view when `outcome` holds one. */
  async function settled<T>(outcome: ImportOutcome<T>): Promise<T | null> {
    if (outcome.kind === "ok") return outcome.view;
    if (outcome.kind === "signed_out") await invalidate(SESSION_DEPENDENCY);
    else if (outcome.kind === "invalid")
      problem = t("hub.import.refused", { message: outcome.message });
    else problem = t(outcome.kind === "denied" ? "hub.import.no_access" : "hub.import.unreachable");
    return null;
  }

  async function save() {
    folderError = null;
    exported = null;
    const target = folder.trim();
    if (!target) {
      folderError = folderProblem("");
      return;
    }
    exporting = true;
    const outcome = await exportSetup(target);
    exporting = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "ok") {
      exported = { ok: true, text: t("hub.import.setup_exported", { path: outcome.view.path }) };
    } else if (outcome.kind === "invalid" && outcome.field === "folder") {
      folderError = folderProblem(outcome.message);
    } else {
      exported = { ok: false, text: t("hub.import.setup_export_failed") };
    }
  }

  async function choose(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    preview = result = null;
    problem = null;
    json = "";
    if (!file) return;
    if (file.size > MAX_SETUP_BYTES) {
      problem = t("hub.import.setup_too_big");
      return;
    }
    checking = true;
    try {
      json = await file.text();
    } catch {
      checking = false;
      problem = t("hub.import.unreadable");
      return;
    }
    importId = newIdempotencyKey();
    const view = await settled(await previewSetup(json));
    checking = false;
    if (view) preview = view;
  }

  async function add() {
    if (!preview || busy) return;
    problem = null;
    adding = true;
    const outcome = await commitSetup(json, importId);
    adding = false;
    const view = await settled(outcome);
    if (!view) {
      if (outcome.kind === "unreachable") problem = t("hub.import.failed");
      return;
    }
    result = view;
    preview = null;
    json = "";
  }
</script>

<div class="setup">
  <h3 class="type-hub-body heading">{t("hub.import.setup_title")}</h3>
  {#if !allowed}
    <p class="type-hub-body muted" data-testid="import-no-access">
      {t("hub.import.setup_no_access")}
    </p>
  {:else}
    <p class="type-hub-body muted">{t("hub.import.setup_intro")}</p>

    <h4 class="type-hub-label">{t("hub.import.setup_export_heading")}</h4>
    <div class="row">
      <label class="field grow type-hub-label">
        <span>{t("hub.import.setup_export_folder")}</span>
        <input
          class="ui-interactive"
          class:invalid={!!folderError}
          type="text"
          autocomplete="off"
          spellcheck="false"
          bind:value={folder}
          disabled={exporting}
          aria-invalid={folderError ? "true" : undefined}
          aria-describedby="setup-export-status"
          data-testid="setup-folder"
        />
      </label>
      <Button
        label="hub.import.setup_export_button"
        variant="secondary"
        disabled={exporting}
        disabledReason="hub.import.setup_exporting"
        onclick={() => void save()}
      />
    </div>
    <div role="status" id="setup-export-status">
      {#if folderError}
        <p class="type-hub-label warn" data-testid="setup-folder-error">
          <span aria-hidden="true">⚠</span>
          {t(folderError)}
        </p>
      {:else if exported}
        <p class="type-hub-label" class:warn={!exported.ok} data-testid="setup-export-status-text">
          {exported.text}
        </p>
      {/if}
    </div>

    <h4 class="type-hub-label">{t("hub.import.setup_import_heading")}</h4>
    <label class="field type-hub-label">
      <span>{t("hub.import.setup_file")}</span>
      <input
        class="ui-interactive file"
        type="file"
        accept=".json,application/json"
        disabled={busy}
        aria-describedby="setup-hint"
        onchange={choose}
        data-testid="setup-file"
      />
    </label>
    <p class="type-hub-label muted" id="setup-hint">{t("hub.import.setup_hint")}</p>

    <div role="status" class="outcome">
      {#if checking}
        <p class="type-hub-label muted">{t("hub.import.checking")}</p>
      {:else if adding}
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
          {t("hub.import.setup_done", { added: result.added, skipped: result.skipped })}
        </p>
      {/if}
    </div>

    {#if preview}
      <div class="preview" data-testid="import-preview">
        <p class="type-hub-body" data-testid="import-summary">
          {t("hub.import.setup_summary", { added: preview.added, skipped: preview.skipped })}
        </p>
        {#if !preview.can_commit}
          <p class="type-hub-label muted" data-testid="setup-nothing">
            {t("hub.import.setup_nothing")}
          </p>
        {/if}
        <ScrollRegion label="hub.import.setup_title">
          <table data-testid="setup-items">
            <thead>
              <tr class="type-hub-label">
                <th scope="col">{t("hub.import.starter_col_item")}</th>
                <th scope="col">{t("hub.import.starter_col_kind")}</th>
                <th scope="col">{t("hub.import.starter_col_result")}</th>
              </tr>
            </thead>
            <tbody class="type-hub-body">
              {#each preview.items as item, i (i)}
                <tr data-testid="setup-item" data-add={item.add}>
                  <td>{item.name}</td>
                  <td>{t(KIND[item.kind] ?? "hub.import.setup_kind_product")}</td>
                  <td>
                    {t(
                      item.add
                        ? "hub.import.starter_will_add"
                        : (SKIP[item.reason ?? ""] ?? "hub.import.starter_skip_invalid"),
                    )}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </ScrollRegion>
        <div class="row">
          <Button
            label="hub.import.setup_add"
            disabled={busy || !preview.can_commit}
            disabledReason={busy ? "hub.import.importing" : "hub.import.commit_nothing"}
            onclick={() => void add()}
          />
        </div>
      </div>
    {/if}
  {/if}
</div>

<style>
  .setup {
    display: grid;
    gap: var(--space-3);
  }

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

  .outcome,
  .preview {
    display: grid;
    gap: var(--space-2);
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
</style>
