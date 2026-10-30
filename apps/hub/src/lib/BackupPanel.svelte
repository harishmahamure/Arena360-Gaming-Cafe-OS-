<script lang="ts">
  import { Button, InlinePin, t } from "@arena/ui";
  import type { BackupExport } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, elevate, loadSettings } from "./auth";
  import {
    BACKUP_EXPORT_KIND,
    KEY_BACKUP_LAST_EXPORT,
    exportDatabase,
    exportedAt,
    parseLastExport,
    problemMessage,
    retryable,
    type ExportOutcome,
    type FolderProblem,
  } from "./backup";
  import { branchLocale } from "./locale.svelte";
  import { formatBytes } from "./media-cache";

  // Settings → System → "Database export": one tap writes the whole database to the backup
  // location. A Counter first has a manager's PIN approve it, in place.
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
  let loadSeq = 0;

  const tz = $derived(branchLocale.current?.tz);

  async function load() {
    const seq = ++loadSeq;
    const result = await loadSettings();
    if (seq !== loadSeq) return;
    if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (result.kind !== "ok") return;
    last = parseLastExport(result.settings.find((s) => s.key === KEY_BACKUP_LAST_EXPORT)?.value);
  }

  function describe(file: BackupExport): string {
    return t("hub.backup.last_value", {
      name: file.file_name,
      size: formatBytes(file.bytes),
      time: exportedAt(file.exported_at, tz),
    });
  }

  /** Tapped: a member with the permission exports at once; a Counter is asked for a PIN. */
  function tap() {
    if (busy || approving) return;
    problem = null;
    notice = null;
    saved = null;
    if (canExport) void run();
    else approving = true;
  }

  async function run(ticket?: string) {
    busy = true;
    problem = null;
    notice = null;
    saved = null;
    let outcome: ExportOutcome;
    try {
      outcome = await exportDatabase(ticket);
    } finally {
      busy = false;
    }
    switch (outcome.kind) {
      case "signed_out":
        return void (await invalidate(SESSION_DEPENDENCY));
      case "ok":
        saved = outcome.export;
        last = outcome.export;
        return;
      case "folder":
        problem = outcome.problem;
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
      case "failed":
        notice = "hub.backup.failed";
        return;
    }
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
    void run(outcome.ticket);
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

  <div class="row">
    <Button
      label={busy ? "hub.backup.exporting" : "hub.backup.export_now"}
      disabled={busy || approving}
      disabledReason={busy ? "hub.backup.exporting" : "hub.settings.system.approving"}
      onclick={tap}
    />
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
            <Button label="hub.backup.try_again" variant="secondary" onclick={tap} />
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
            <Button label="hub.backup.try_again" variant="secondary" onclick={tap} />
          </div>
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
