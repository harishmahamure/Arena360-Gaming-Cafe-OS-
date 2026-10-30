<script lang="ts">
  import { StatusBadge, t } from "@arena/ui";
  import { agoOf } from "./backup-status";
  import { backup, backupBehind, hubNow } from "./backup-status.svelte";

  // The header's backup line (Story 9.16): muted "Backed up 4 min ago · 3 pending", or a warning
  // badge "Backup behind" once the last upload is older than twice the interval. It is a link to
  // System -> Backup; the tooltip and the accessible name also give the last summary and the
  // batches waiting. Free shows nothing.

  const status = $derived(backup.status);
  const behind = $derived(backupBehind());
  const when = (at: number | null) => {
    const a = agoOf(at, hubNow());
    return t(a.key, { n: a.n });
  };
  const line = $derived.by(() => {
    if (!status) return "";
    if (status.first_backup_pending || status.last_upload_at === null) {
      return t("hub.header.backup.first");
    }
    const ago = when(status.last_upload_at);
    return status.pending_batches > 0
      ? t("hub.header.backup.line_pending", { when: ago, count: status.pending_batches })
      : t("hub.header.backup.line", { when: ago });
  });
  const detail = $derived.by(() => {
    if (!status) return "";
    const pending =
      status.pending_batches === 1
        ? t("hub.header.backup.pending_one")
        : t("hub.header.backup.pending_many", { count: status.pending_batches });
    return t("hub.header.backup.detail", {
      upload: when(status.last_upload_at),
      summary: when(status.last_summary_at),
      pending,
    });
  });
</script>

{#if status?.cloud}
  <a
    class="backup-chip ui-interactive type-hub-label"
    class:behind
    href="/system/backup"
    title={detail}
    aria-label={t("hub.header.backup.label", {
      status: behind ? t("hub.header.backup.behind") : line,
      detail,
    })}
    data-testid="hub-backup-status"
    data-behind={behind ? "true" : "false"}
  >
    {#if behind}
      <StatusBadge state="warning" text={t("hub.header.backup.behind")} />
    {:else}
      <span aria-hidden="true">{line}</span>
    {/if}
  </a>
{/if}

<style>
  .backup-chip {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-2);
    color: var(--text-muted);
    text-decoration: none;
    white-space: nowrap;
  }
</style>
