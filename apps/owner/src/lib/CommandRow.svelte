<script lang="ts">
  import { Button, StatusBadge, t } from "@arena/ui";
  import {
    again,
    branchOf,
    cmd,
    queuedText,
    rowKey,
    sendHeld,
    stateOf,
    whatOf,
    type Row,
  } from "./commands.svelte";

  let { row, now }: { row: Row; now: number } = $props();

  const view = $derived(stateOf(row, now));
  const branch = $derived(
    branchOf(row.branch_id)?.name ??
      (row.branch_id === "all" ? t("owner.cmd.scope.all") : row.branch_id),
  );
  const key = $derived(rowKey(row));
  const status = $derived(row.local ? `local-${row.local.state}` : row.status); // i18n-ignore
  const scope = $derived(
    row.group_id && row.branch_id !== "all" ? t("owner.cmd.scope.all_at", { branch }) : branch,
  );
</script>

<li class="row" data-component="command-row" data-status={status}>
  <div class="what">
    <span class="type-hub-label">{whatOf(row.kind, row.payload)}</span>
    <span class="meta type-hub-micro">{scope} · {queuedText(row, now)}</span>
  </div>
  <div class="state">
    <StatusBadge tone={view.tone} text={view.text} />
    {#if view.action}
      <Button
        label={view.action === "retry"
          ? "common.retry"
          : view.action === "send"
            ? "owner.cmd.send"
            : "owner.cmd.resend"}
        variant="secondary"
        onclick={() => (row.local && view.action === "send" ? sendHeld(row.local) : again(row))}
        disabled={cmd.again[key] === "sending"}
        disabledReason="owner.cmd.sending"
      />
    {/if}
  </div>
  {#if cmd.again[key] === "failed"}
    <p class="failed type-hub-micro" role="alert">{t("owner.cmd.again.failed")}</p>
  {/if}
</li>

<style>
  .row {
    --status-badge-background: var(--surface);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-4) var(--space-5);
    background-color: var(--surface);
    border-bottom: 1px solid var(--line);
  }
  .what {
    display: grid;
    gap: var(--space-1);
    min-width: 0;
  }
  .meta {
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }
  .state {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
  .failed {
    flex-basis: 100%;
    margin: 0;
    color: var(--danger);
  }
</style>
