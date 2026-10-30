<script lang="ts">
  import { StatusBadge, TierLock, formatDateTime, t } from "@arena/ui";
  import type { GstArchiveEntry } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY } from "../../../lib/auth";
  import { agoOf, errorMessage, errorSource } from "../../../lib/backup-status";
  import { backup, backupBehind, hubNow, refreshBackup } from "../../../lib/backup-status.svelte";
  import { loadGstArchives } from "../../../lib/gst-archive";
  import { branchLocale } from "../../../lib/locale.svelte";

  // System -> Backup: how far the cloud backup has got (Story 9.16) and the yearly GST archives the
  // Hub has saved to the cloud. Both are read from the Hub's own logs, so they are the same
  // whether or not the cloud can be reached now. A Free Hub shows local exports only.

  let archives: GstArchiveEntry[] | null = $state(null);
  let problem: "unreachable" | "denied" | null = $state(null);
  let statusProblem = $state(false);

  const status = $derived(backup.status);
  const behind = $derived(backupBehind());
  const ago = (at: number | null) => {
    const a = agoOf(at, hubNow());
    return t(a.key, { n: a.n });
  };

  const tz = $derived(branchLocale.current?.tz);

  onMount(async () => {
    const read = await refreshBackup();
    if (read.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    statusProblem = read.kind !== "ok";
    if (status && !status.cloud) return;
    const outcome = await loadGstArchives();
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "ok") archives = outcome.archives;
    else problem = outcome.kind;
  });
</script>

<section class="backup" aria-labelledby="backup-title">
  <h2 class="type-hub-title" id="backup-title">{t("hub.backup.section_title")}</h2>

  {#if statusProblem && !status}
    <p class="type-hub-body" role="status" data-testid="cloud-unreadable">
      {t("hub.backup.cloud.unreachable")}
    </p>
  {:else if status && !status.cloud}
    <div class="cloud" data-testid="cloud-free">
      <h3 class="type-hub-label">{t("hub.backup.cloud.title")}</h3>
      <p class="type-hub-body">{t("hub.backup.cloud.free")}</p>
      <TierLock tier="Basic" />
    </div>
    <div class="cloud" data-testid="local-exports">
      <h3 class="type-hub-label">{t("hub.backup.local.title")}</h3>
      <p class="type-hub-body">{t("hub.backup.local.intro")}</p>
      <a class="link ui-interactive type-hub-label" href="/settings/system"
        >{t("hub.backup.local.open")}</a
      >
    </div>
  {:else if status}
    <div class="cloud" data-testid="cloud-backup">
      <h3 class="type-hub-label">{t("hub.backup.cloud.title")}</h3>
      <p class="type-hub-body">{t("hub.backup.cloud.intro")}</p>
      {#if status.first_backup_pending}
        <p class="type-hub-body" role="status" data-testid="cloud-first-pending">
          {t("hub.backup.cloud.first_pending")}
        </p>
      {:else if behind}
        <p class="type-hub-body" role="status" data-testid="cloud-behind">
          <StatusBadge state="warning" text={t("hub.header.backup.behind")} />
          {t("hub.backup.cloud.behind")}
        </p>
      {/if}
      <dl class="facts type-hub-body">
        <dt>{t("hub.backup.cloud.last_upload")}</dt>
        <dd data-testid="cloud-last-upload">{ago(status.last_upload_at)}</dd>
        <dt>{t("hub.backup.cloud.last_summary")}</dt>
        <dd data-testid="cloud-last-summary">{ago(status.last_summary_at)}</dd>
        <dt>{t("hub.backup.cloud.last_snapshot")}</dt>
        <dd data-testid="cloud-last-snapshot">
          {status.last_snapshot_date
            ? t("hub.backup.cloud.snapshot_for", {
                when: ago(status.last_snapshot_at),
                date: status.last_snapshot_date,
              })
            : ago(null)}
        </dd>
        <dt>{t("hub.backup.cloud.last_keys")}</dt>
        <dd data-testid="cloud-last-keys">{ago(status.last_keys_at)}</dd>
        <dt>{t("hub.backup.cloud.last_gst")}</dt>
        <dd data-testid="cloud-last-gst">{ago(status.last_gst_archive_at)}</dd>
        <dt>{t("hub.backup.cloud.pending")}</dt>
        <dd data-testid="cloud-pending">{status.pending_batches}</dd>
      </dl>
      {#if status.error}
        <p class="type-hub-body" role="status" data-testid="cloud-error">
          <strong>{t("hub.backup.cloud.error_title")}</strong>
          {t("hub.backup.cloud.error_line", {
            source: t(errorSource(status.error)),
            problem: t(errorMessage(status.error)),
          })}
        </p>
      {/if}
    </div>
  {/if}

  {#if !status || status.cloud}
    <div class="gst" data-testid="gst-archives">
      <h3 class="type-hub-label">{t("hub.backup.gst.title")}</h3>
      <p class="type-hub-body">{t("hub.backup.gst.intro")}</p>
      {#if problem === "denied"}
        <p class="type-hub-body" role="status" data-testid="gst-denied">
          {t("hub.backup.gst.denied")}
        </p>
      {:else if problem}
        <p class="type-hub-body" role="status" data-testid="gst-unreadable">
          {t("hub.backup.gst.unreadable")}
        </p>
      {:else if archives && archives.length === 0}
        <p class="type-hub-body" data-testid="gst-none">{t("hub.backup.gst.none")}</p>
      {:else if archives}
        <ul class="years">
          {#each archives as a (a.fy)}
            <!-- i18n-ignore -->
            <li class="year" data-testid={`gst-archive-${a.fy}`}>
              <StatusBadge tone="ok" text={t("hub.backup.gst.saved", { year: a.label })} />
              <span class="type-hub-body"
                >{t("hub.backup.gst.invoices", { count: a.invoices })}</span
              >
              <span class="type-hub-body"
                >{t("hub.backup.gst.credit_notes", { count: a.credit_notes })}</span
              >
              {#if a.orphaned > 0}
                <span class="type-hub-body"
                  >{t("hub.backup.gst.orphaned", { count: a.orphaned })}</span
                >
              {/if}
              <span class="type-hub-body tabular-nums"
                >{t("hub.backup.gst.built", { when: formatDateTime(a.built_at, tz) })}</span
              >
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</section>

<style>
  .backup {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 60ch;
  }

  h2,
  h3,
  p {
    margin: 0;
  }

  .gst,
  .cloud {
    display: grid;
    justify-items: start;
    gap: var(--space-3);
  }

  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-2) var(--space-5);
    margin: 0;
  }

  .facts dt {
    color: var(--text-muted);
  }

  .facts dd {
    margin: 0;
  }

  .link {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    color: var(--text);
    text-decoration: underline;
  }

  .years {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .year {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
</style>
