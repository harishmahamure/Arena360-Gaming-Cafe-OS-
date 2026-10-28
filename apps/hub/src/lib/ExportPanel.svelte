<script lang="ts" module>
  /** The folder last used, kept while the Hub tab stays open so each report starts from it. */
  let lastFolder = "";
</script>

<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { ReportExportRequest } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, loadSettings } from "./auth";
  import { BACKUP_DIR_KEY, exportReport, folderProblem } from "./reports";

  type Props = {
    /** The report id the Hub exports (`daily-sales`, `stock-moves`, ...). */
    report: string;
    /** What the screen shows: its dates, month, shift or filters. */
    selection?: Omit<ReportExportRequest, "report" | "format" | "folder">;
    /** Turns the buttons off, with `reason` shown, while there is nothing on screen to export. */
    unavailable?: boolean;
  };

  let { report, selection = {}, unavailable = false }: Props = $props();

  let folder = $state(lastFolder);
  let busy = $state(false);
  let folderError: MessageKey | null = $state(null);
  let problem: MessageKey | null = $state(null);
  let saved = $state("");

  onMount(async () => {
    if (folder) return;
    const outcome = await loadSettings();
    if (outcome.kind !== "ok") return;
    const dir = outcome.settings.find((s) => s.key === BACKUP_DIR_KEY)?.value ?? "";
    // A tree URI (Android) is not a path the Hub can write a file into.
    if (!folder && dir && !dir.startsWith("content://")) folder = dir;
  });

  async function run(format: "csv" | "pdf") {
    folderError = problem = null;
    saved = "";
    const target = folder.trim();
    if (!target) {
      folderError = "hub.reports.export.folder_required";
      return;
    }
    busy = true;
    const outcome = await exportReport({ ...selection, report, format, folder: target });
    busy = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "ok") {
      lastFolder = target;
      saved = outcome.view.path;
    } else if (outcome.kind === "invalid") {
      if (outcome.field === "folder") folderError = folderProblem(outcome.reason);
      else problem = "hub.reports.export.invalid";
    } else {
      problem = outcome.kind === "denied" ? "hub.reports.no_access" : "hub.reports.export.failed";
    }
  }
</script>

<section class="export" aria-labelledby="export-title" data-testid="export-panel">
  <h2 class="type-hub-body" id="export-title">{t("hub.reports.export.title")}</h2>
  <div class="row">
    <label class="field grow type-hub-label">
      <span>{t("hub.reports.export.folder")}</span>
      <input
        class="ui-interactive"
        class:invalid={!!folderError}
        type="text"
        autocomplete="off"
        spellcheck="false"
        bind:value={folder}
        disabled={busy}
        aria-invalid={folderError ? "true" : undefined}
        aria-describedby="export-hint export-status"
        data-testid="export-folder"
      />
    </label>
    <Button
      label="hub.reports.export.csv"
      variant="secondary"
      disabled={busy || unavailable}
      disabledReason={busy ? "hub.reports.export.writing" : "hub.reports.export.nothing"}
      onclick={() => run("csv")}
    />
    <Button
      label="hub.reports.export.pdf"
      variant="secondary"
      disabled={busy || unavailable}
      disabledReason={busy ? "hub.reports.export.writing" : "hub.reports.export.nothing"}
      onclick={() => run("pdf")}
    />
  </div>
  <p class="type-hub-label note" id="export-hint">{t("hub.reports.export.hint")}</p>
  <div role="status" id="export-status">
    {#if folderError}
      <p class="type-hub-label warn" data-testid="export-folder-error">
        <span aria-hidden="true">⚠</span>
        {t(folderError)}
        {t("hub.reports.export.another")}
      </p>
    {:else if problem}
      <p class="type-hub-label warn" data-testid="export-problem">
        <span aria-hidden="true">⚠</span>
        {t(problem)}
      </p>
    {:else if busy}
      <p class="type-hub-label note">{t("hub.reports.export.writing")}</p>
    {:else if saved}
      <p class="type-hub-body" data-testid="export-saved">
        {t("hub.reports.export.saved", { path: saved })}
      </p>
    {/if}
  </div>
</section>

<style>
  .export {
    display: grid;
    gap: var(--space-2);
    padding-block-start: var(--space-3);
    border-block-start: 1px solid var(--border-strong);
  }

  h2,
  p {
    margin: 0;
  }

  .note {
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

  input {
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
</style>
