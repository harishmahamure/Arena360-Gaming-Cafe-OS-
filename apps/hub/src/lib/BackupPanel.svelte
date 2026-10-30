<script lang="ts">
  import { Button, InlinePin, t } from "@arena/ui";
  import type { BackupExport, EverythingExported, ExportEntry } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, elevate, loadSettings } from "./auth";
  import {
    BACKUP_EXPORT_KIND,
    KEY_BACKUP_LAST_EXPORT,
    exportDatabase,
    exportEverything,
    exportList,
    exportedAt,
    parseLastExport,
    problemMessage,
    retryable,
    type EverythingOutcome,
    type ExportOutcome,
    type FolderProblem,
  } from "./backup";
  import { MAX_DAYS, addDays, businessDayNow, daySpan } from "./reports";
  import { branchLocale } from "./locale.svelte";
  import { formatBytes } from "./media-cache";

  // Settings → System → "Database export": one tap writes the whole database to the backup
  // location. A Counter first has a manager's PIN approve it, in place. "Export everything"
  // (Story 9.6) writes the database, every report for a range as CSV and a README into one folder.
  let {
    canExport,
    onchoose,
  }: {
    /** The signed-in member holds `backup.export` (a Counter does not and asks for a PIN). */
    canExport: boolean;
    /** Brings the backup location field (or the folder picker) forward. */
    onchoose: () => void;
  } = $props();

  let last: BackupExport | null = $state(null);
  let busy = $state(false);
  let approving = $state(false);
  let problem: FolderProblem | null = $state(null);
  let notice: MessageKey | null = $state(null);
  let saved: BackupExport | null = $state(null);
  let savedAll: EverythingExported | null = $state(null);
  /** What a PIN approval is for. */
  let approvingFor: "database" | "everything" = $state("database");
  let from = $state("");
  let to = $state("");
  let loadSeq = 0;

  const tz = $derived(branchLocale.current?.tz);
  const entries = $derived(exportList(last));
  const span = $derived(daySpan(from, to));
  const rangeValid = $derived(span >= 1 && span <= MAX_DAYS);
  const rangeMessage: MessageKey | null = $derived(
    !from || !to || rangeValid
      ? null
      : span < 1
        ? "hub.reports.range_order"
        : "hub.reports.range_long",
  );

  // The last 30 days by default, ending with the branch's business day today.
  $effect(() => {
    const locale = branchLocale.current;
    if (!locale) return;
    untrack(() => {
      if (from || to) return;
      const today = businessDayNow(locale, Date.now());
      from = addDays(today, -29);
      to = today;
    });
  });

  async function load() {
    const seq = ++loadSeq;
    const result = await loadSettings();
    if (seq !== loadSeq) return;
    if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (result.kind !== "ok") return;
    last = parseLastExport(result.settings.find((s) => s.key === KEY_BACKUP_LAST_EXPORT)?.value);
  }

  function describeEntry(entry: ExportEntry): string {
    return t("hub.backup.entry", {
      name: entry.file_name,
      size: formatBytes(entry.bytes),
      time: exportedAt(entry.exported_at, tz),
      by: entry.by ?? t("hub.backup.auto"),
    });
  }

  function describe(file: BackupExport): string {
    return t("hub.backup.last_value", {
      name: file.file_name,
      size: formatBytes(file.bytes),
      time: exportedAt(file.exported_at, tz),
    });
  }

  function clear() {
    problem = null;
    notice = null;
    saved = null;
    savedAll = null;
  }

  /** Tapped: a member with the permission exports at once; a Counter is asked for a PIN. */
  function tap(what: "database" | "everything" = "database") {
    if (busy || approving) return;
    if (what === "everything" && !rangeValid) return;
    clear();
    approvingFor = what;
    if (canExport) void run(what);
    else approving = true;
  }

  /** The cause shared by both exports. */
  function explain(outcome: Exclude<ExportOutcome, { kind: "ok" }>) {
    switch (outcome.kind) {
      case "folder":
        problem = outcome.problem;
        return;
      case "range":
        notice = "hub.backup.everything.range";
        return;
      case "pin_required":
        notice = "hub.backup.needs_pin";
        approving = true;
        return;
      case "denied":
        notice = "hub.backup.denied";
        return;
      case "not_primary":
        notice = "hub.backup.not_primary";
        return;
      case "signed_out":
        return;
      case "failed":
        notice = "hub.backup.failed";
        return;
    }
  }

  async function run(what: "database" | "everything", ticket?: string) {
    busy = true;
    clear();
    let outcome: ExportOutcome | EverythingOutcome;
    try {
      outcome =
        what === "everything"
          ? await exportEverything(from, to, ticket)
          : await exportDatabase(ticket);
    } finally {
      busy = false;
    }
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind !== "ok") return explain(outcome);
    if ("done" in outcome) {
      savedAll = outcome.done;
      last = outcome.done.export;
    } else {
      saved = outcome.export;
      last = outcome.export;
    }
    // The Hub kept the list of the exports before this one with the record.
    void load();
  }

  async function approve(pin: string) {
    const outcome = await elevate([{ intent_kind: BACKUP_EXPORT_KIND }], pin);
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
    void run(approvingFor, outcome.ticket);
    return { ok: true } as const;
  }

  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      if (!busy && branchLocale.changedKeys.includes(KEY_BACKUP_LAST_EXPORT)) void load();
    });
  });

  onMount(() => void load());
</script>

<section class="backup" aria-labelledby="settings-backup-title" data-testid="settings-backup">
  <h3 class="type-hub-title" id="settings-backup-title">{t("hub.backup.title")}</h3>
  <p class="intro type-hub-body">{t("hub.backup.intro")}</p>

  <p class="last type-hub-body" data-testid="backup-last">
    <span class="label type-hub-label">{t("hub.backup.last")}</span>
    {#if last}
      <span data-testid="backup-last-value">{describe(last)}</span>
    {:else}
      <span class="muted" data-testid="backup-never">{t("hub.backup.never")}</span>
    {/if}
  </p>

  {#if entries.length > 0}
    <div class="recent" data-testid="backup-recent">
      <h4 class="type-hub-label">{t("hub.backup.recent")}</h4>
      <ul class="type-hub-body">
        {#each entries as entry (entry.file_name + entry.exported_at)}
          <li data-testid="backup-entry">{describeEntry(entry)}</li>
        {/each}
      </ul>
    </div>
  {/if}

  <div class="row">
    <Button
      label={busy ? "hub.backup.exporting" : "hub.backup.export_now"}
      disabled={busy || approving}
      disabledReason={busy ? "hub.backup.exporting" : "hub.settings.system.approving"}
      onclick={() => tap()}
    />
  </div>

  <div class="everything" data-testid="backup-everything">
    <h4 class="type-hub-label">{t("hub.backup.everything.title")}</h4>
    <p class="intro type-hub-body">{t("hub.backup.everything.intro")}</p>
    <div class="row">
      <label class="field type-hub-label">
        <span>{t("hub.reports.from")}</span>
        <input class="ui-interactive" type="date" bind:value={from} data-testid="backup-from" />
      </label>
      <label class="field type-hub-label">
        <span>{t("hub.reports.to")}</span>
        <input class="ui-interactive" type="date" bind:value={to} data-testid="backup-to" />
      </label>
    </div>
    {#if rangeMessage}
      <p class="type-hub-label warn" role="alert" data-testid="backup-range-problem">
        {t(rangeMessage)}
      </p>
    {/if}
    <div class="row">
      <Button
        label={busy ? "hub.backup.exporting" : "hub.backup.everything.button"}
        variant="secondary"
        disabled={busy || approving || !rangeValid}
        disabledReason={busy
          ? "hub.backup.exporting"
          : approving
            ? "hub.settings.system.approving"
            : (rangeMessage ?? "hub.backup.everything.range")}
        onclick={() => tap("everything")}
      />
    </div>
  </div>

  {#if approving}
    <div class="approval" data-testid="backup-approval">
      <InlinePin
        message="hub.backup.pin_prompt"
        onsubmit={approve}
        oncancel={() => (approving = false)}
      />
    </div>
  {/if}

  <div class="result" role="status" aria-live="polite" data-testid="backup-status">
    {#if busy}
      <p class="type-hub-label muted">{t("hub.backup.exporting")}</p>
    {:else if problem}
      <div class="cause" data-testid="backup-problem">
        <p class="type-hub-label warn">
          <span aria-hidden="true">⚠</span>
          <span>{t(problemMessage(problem))}</span>
        </p>
        <div class="row">
          {#if retryable(problem)}
            <Button
              label="hub.backup.try_again"
              variant="secondary"
              onclick={() => tap(approvingFor)}
            />
          {/if}
          <Button label="hub.backup.choose_location" variant="secondary" onclick={onchoose} />
        </div>
      </div>
    {:else if notice}
      <div class="cause" data-testid="backup-notice">
        <p class="type-hub-label warn">
          <span aria-hidden="true">⚠</span>
          <span>{t(notice)}</span>
        </p>
        {#if notice === "hub.backup.failed"}
          <div class="row">
            <Button
              label="hub.backup.try_again"
              variant="secondary"
              onclick={() => tap(approvingFor)}
            />
          </div>
        {/if}
      </div>
    {:else if savedAll}
      <div data-testid="backup-saved-everything">
        <p class="type-hub-body">
          {t("hub.backup.everything.saved", {
            name: savedAll.folder_name,
            n: String(savedAll.reports.length),
          })}
        </p>
        {#if savedAll.skipped.length > 0}
          <p class="type-hub-label warn" data-testid="backup-skipped">
            {t("hub.backup.everything.skipped", { reports: savedAll.skipped.join(", ") })}
          </p>
        {/if}
      </div>
    {:else if saved}
      <p class="type-hub-body" data-testid="backup-saved">
        {t("hub.backup.saved", { name: saved.file_name })}
      </p>
    {/if}
  </div>
</section>

<style>
  .backup {
    display: grid;
    gap: var(--space-3);
    max-width: 40rem;
    padding-top: var(--space-4);
    border-top: 1px solid var(--border-strong);
  }

  h3,
  .intro,
  .last,
  .result p {
    margin: 0;
  }

  .intro,
  .muted {
    color: var(--text-muted);
  }

  .last {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    overflow-wrap: anywhere;
  }

  .recent {
    display: grid;
    gap: var(--space-2);
  }

  .recent h4,
  .recent ul {
    margin: 0;
  }

  .recent ul {
    padding-inline-start: var(--space-5);
    overflow-wrap: anywhere;
  }

  .label {
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .everything {
    display: grid;
    gap: var(--space-2);
  }

  .everything h4 {
    margin: 0;
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  .approval,
  .cause {
    display: grid;
    gap: var(--space-2);
  }

  .result {
    min-height: 1lh;
  }

  .warn {
    display: flex;
    gap: var(--space-2);
    color: var(--text-field-error-text);
  }
</style>
