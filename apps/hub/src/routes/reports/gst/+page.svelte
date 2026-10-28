<script lang="ts">
  import { Button, formatMoney, t } from "@arena/ui";
  import type { GstMonth } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { SESSION_DEPENDENCY, hasPermission, loadSettings } from "../../../lib/auth";
  import { branchLocale } from "../../../lib/locale.svelte";
  import { sessionStaff } from "../../../lib/operating";
  import ScrollRegion from "../../../lib/ScrollRegion.svelte";
  import {
    businessMonthNow,
    exportGst,
    folderProblem,
    isGstBranch,
    loadGstMonth,
  } from "../../../lib/reports";

  const SECTION: Record<string, MessageKey> = {
    b2b: "hub.gst.section.b2b",
    b2cs: "hub.gst.section.b2cs",
    cdnr: "hub.gst.section.cdnr",
    hsn: "hub.gst.section.hsn",
  };
  /** The settings key of the backup folder; a plain path there is a good first guess. */
  const BACKUP_KEY = "system.backup_dir";

  const locale = $derived(branchLocale.current);
  const allowed = $derived(hasPermission(sessionStaff(page.data), "report.view"));

  let month = $state("");
  let view = $state<GstMonth | null>(null);
  let notGst = $state(false);
  let problem: MessageKey | null = $state(null);
  let loading = $state(false);
  let folder = $state("");
  let exporting = $state(false);
  let folderError: MessageKey | null = $state(null);
  let saved = $state("");
  let exportProblem: MessageKey | null = $state(null);
  let turn = 0;

  async function load() {
    const mine = ++turn;
    loading = true;
    saved = "";
    const outcome = await loadGstMonth(month);
    if (mine !== turn) return;
    loading = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    notGst = outcome.kind === "invalid" && outcome.field === "tax";
    problem =
      outcome.kind === "ok" || notGst
        ? null
        : outcome.kind === "invalid"
          ? "hub.gst.invalid_month"
          : outcome.kind === "denied"
            ? "hub.reports.no_access"
            : "hub.gst.failed";
    if (outcome.kind === "ok") view = outcome.view;
  }

  async function prefillFolder() {
    const outcome = await loadSettings();
    if (outcome.kind !== "ok") return;
    notGst = !isGstBranch(outcome.settings);
    const dir = outcome.settings.find((s) => s.key === BACKUP_KEY)?.value ?? "";
    // A tree URI (Android) is not a path the Hub can write a file into.
    if (!folder && dir && !dir.startsWith("content://")) folder = dir;
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    folderError = exportProblem = null;
    saved = "";
    if (!folder.trim()) {
      folderError = "hub.gst.folder_required";
      return;
    }
    exporting = true;
    const outcome = await exportGst(month, folder.trim());
    exporting = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "ok") {
      saved = outcome.view.path;
      view = outcome.view.month;
    } else if (outcome.kind === "invalid") {
      if (outcome.field === "folder") folderError = folderProblem(outcome.reason);
      else exportProblem = outcome.field === "tax" ? "hub.gst.not_gst" : "hub.gst.invalid_month";
    } else {
      exportProblem = outcome.kind === "denied" ? "hub.reports.no_access" : "hub.gst.export_failed";
    }
  }

  // Opens on the current month once the branch locale (its time zone and day start) is in.
  $effect(() => {
    if (locale && allowed && !month) {
      month = businessMonthNow(locale, Date.now());
      untrack(() => {
        void load();
        void prefillFolder();
      });
    }
  });

  function show(event: SubmitEvent) {
    event.preventDefault();
    if (/^\d{4}-\d{2}$/.test(month)) void load();
    else problem = "hub.gst.invalid_month";
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.reports.gst") })}</title>
</svelte:head>

<section class="report" data-testid="gst-screen">
  <h1 class="type-hub-numeric">{t("hub.gst.title")}</h1>
  <p class="type-hub-label"><a href="/reports">{t("hub.reports.back")}</a></p>

  {#if !allowed}
    <p class="type-hub-body note" role="status" data-testid="reports-denied">
      {t("hub.reports.no_access")}
    </p>
  {:else if notGst}
    <p class="type-hub-body note" role="status" data-testid="gst-not-gst">
      {t("hub.gst.not_gst")}
    </p>
  {:else}
    <form class="month" onsubmit={show}>
      <label class="field type-hub-label">
        <span>{t("hub.gst.month")}</span>
        <input class="ui-interactive" type="month" bind:value={month} data-testid="gst-month" />
      </label>
      <Button label="hub.gst.show" type="submit" />
    </form>

    <div role="status">
      {#if problem}
        <p class="type-hub-label warn" data-testid="report-problem">
          <span aria-hidden="true">⚠</span>
          {t(problem)}
        </p>
      {:else if loading}
        <p class="type-hub-label note">{t("hub.reports.loading")}</p>
      {/if}
    </div>

    {#if view && locale}
      <p class="type-hub-body" data-testid="gst-counts">
        {t("hub.gst.counts", { invoices: view.invoices, notes: view.credit_notes })}
      </p>
      {#if view.empty}
        <p class="type-hub-body note" data-testid="gst-empty">
          {t("hub.gst.none", { month: view.month })}
        </p>
      {/if}
      {#if view.untaxed > 0}
        <p class="type-hub-label note" data-testid="gst-untaxed">
          {t("hub.gst.untaxed", { count: view.untaxed })}
        </p>
      {/if}
      {#if view.unnumbered > 0}
        <p class="type-hub-label note" data-testid="gst-unnumbered">
          {t("hub.gst.unnumbered", { count: view.unnumbered })}
        </p>
      {/if}

      <h2 class="type-hub-body">{t("hub.gst.sections")}</h2>
      <ScrollRegion label="hub.gst.sections">
        <table class="type-hub-body" data-testid="gst-sections">
          <caption class="sr-only">{t("hub.gst.sections")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("hub.gst.col.section")}</th>
              <th scope="col" class="num">{t("hub.gst.col.documents")}</th>
              <th scope="col" class="num">{t("hub.gst.col.taxable")}</th>
              <th scope="col" class="num">{t("hub.gst.col.cgst")}</th>
              <th scope="col" class="num">{t("hub.gst.col.sgst")}</th>
              <th scope="col" class="num">{t("hub.gst.col.igst")}</th>
            </tr>
          </thead>
          <tbody>
            {#each view.sections as s (s.key)}
              <tr data-testid="gst-section-{s.key}">
                <th scope="row">{t(SECTION[s.key] ?? "hub.gst.section.hsn")}</th>
                <td class="num tabular-nums">{s.documents}</td>
                <td class="num tabular-nums">{formatMoney(s.taxable_minor, locale)}</td>
                <td class="num tabular-nums">{formatMoney(s.central_minor, locale)}</td>
                <td class="num tabular-nums">{formatMoney(s.state_minor, locale)}</td>
                <td class="num tabular-nums">{formatMoney(s.integrated_minor, locale)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </ScrollRegion>

      <h2 class="type-hub-body">{t("hub.gst.export_title")}</h2>
      <p class="type-hub-label note">{t("hub.gst.files")}</p>
      <form class="export" onsubmit={save} novalidate>
        <label class="field grow type-hub-label">
          <span>{t("hub.gst.folder")}</span>
          <input
            class="ui-interactive"
            class:invalid={!!folderError}
            type="text"
            autocomplete="off"
            spellcheck="false"
            bind:value={folder}
            disabled={exporting}
            aria-invalid={folderError ? "true" : undefined}
            aria-describedby="gst-folder-hint gst-folder-error"
            data-testid="gst-folder"
          />
        </label>
        <Button
          label="hub.gst.export"
          type="submit"
          disabled={exporting}
          disabledReason="hub.gst.exporting"
        />
      </form>
      <p class="type-hub-label note" id="gst-folder-hint">{t("hub.gst.folder_hint")}</p>
      <div role="status" id="gst-folder-error">
        {#if folderError}
          <p class="type-hub-label warn" data-testid="gst-folder-error">
            <span aria-hidden="true">⚠</span>
            {t(folderError)}
          </p>
        {:else if exportProblem}
          <p class="type-hub-label warn" data-testid="gst-export-problem">
            <span aria-hidden="true">⚠</span>
            {t(exportProblem)}
          </p>
        {:else if exporting}
          <p class="type-hub-label note">{t("hub.gst.exporting")}</p>
        {:else if saved}
          <p class="type-hub-body" data-testid="gst-saved">{t("hub.gst.saved", { path: saved })}</p>
        {/if}
      </div>
    {/if}
  {/if}
</section>

<style>
  .report {
    display: grid;
    gap: var(--space-3);
  }

  h1,
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

  .month,
  .export {
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

  table {
    inline-size: 100%;
    max-inline-size: 56rem;
    border-collapse: collapse;
  }

  th,
  td {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border-strong);
    text-align: start;
    vertical-align: middle;
    font-weight: inherit;
  }

  .num {
    text-align: end;
  }

  .sr-only {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
