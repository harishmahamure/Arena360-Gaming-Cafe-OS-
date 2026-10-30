<script lang="ts">
  import { Button, Card, StatusBadge, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { cancel, cmd, confirm, whatOf } from "./commands.svelte";

  const REASONS: Record<string, MessageKey> = {
    FEATURE_NOT_ENTITLED: "owner.cmd.reason.entitled",
    BRANCH_UPDATE_REQUIRED: "owner.cmd.reason.update",
  };

  let heading: HTMLHeadingElement | undefined = $state();

  const draft = $derived(cmd.draft);
  const targets = $derived(cmd.preview?.targets ?? []);
  const offered = $derived(targets.filter((x) => x.offered));
  const scope = $derived(
    draft?.branch_id === "all" ? t("owner.cmd.scope.all") : (targets[0]?.name ?? ""),
  );

  // The preview opens beside the form that asked for it: move the reader to it.
  $effect(() => {
    if (draft) void tick().then(() => heading?.focus());
  });
</script>

{#if draft}
  <Card>
    <section class="preview" aria-labelledby="cmd-preview-title" data-component="command-preview">
      <h2 id="cmd-preview-title" class="type-hub-title" tabindex="-1" bind:this={heading}>
        {t("owner.cmd.preview.title")}
      </h2>
      <p class="summary">
        {t("owner.cmd.preview.summary", { what: whatOf(draft.kind, draft.payload), scope })}
      </p>
      {#if cmd.previewing}
        <p>{t("owner.cmd.preview.checking")}</p>
      {:else if cmd.previewFailed}
        <p role="alert">{t("owner.cmd.preview.failed")}</p>
      {:else}
        <h3 class="type-hub-label">{t("owner.cmd.preview.targets")}</h3>
        <ul class="targets">
          {#each targets as x (x.branch_id)}
            <li class="target" class:muted={!x.offered}>
              <span class="name">{x.name}</span>
              <StatusBadge
                tone={x.online ? "ok" : "muted"}
                label={x.online ? "owner.cmd.preview.online" : "owner.cmd.preview.offline"}
              />
              {#if !x.offered}
                <span class="note type-hub-micro"
                  >{t("owner.cmd.preview.not_offered")} · {t(
                    REASONS[x.error?.code ?? ""] ?? "owner.cmd.reason.other",
                  )}</span
                >
              {:else if !x.online}
                <span class="note type-hub-micro">{t("owner.cmd.preview.offline_note")}</span>
              {/if}
            </li>
          {/each}
        </ul>
        {#if !offered.length}<p>{t("owner.cmd.preview.none")}</p>{/if}
      {/if}
      {#if cmd.sendError}<p class="error" role="alert">{t(cmd.sendError)}</p>{/if}
      <div class="actions">
        <Button
          label="owner.cmd.send"
          onclick={confirm}
          disabled={cmd.previewing || cmd.previewFailed || cmd.sending || !offered.length}
          disabledReason={cmd.sending
            ? "owner.cmd.sending"
            : cmd.previewing
              ? "owner.cmd.preview.checking"
              : cmd.previewFailed
                ? "owner.cmd.preview.failed"
                : "owner.cmd.send.none"}
        />
        <Button label="owner.cmd.cancel" variant="secondary" onclick={cancel} />
      </div>
    </section>
  </Card>
{/if}

<style>
  .preview {
    --status-badge-background: var(--surface);
    display: grid;
    gap: var(--space-3);
  }
  h2,
  h3,
  p {
    margin: 0;
  }
  .summary {
    color: var(--text-muted);
  }
  .targets {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-3);
  }
  .target {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
  }
  .target.muted .name {
    color: var(--text-muted);
  }
  .name {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .note {
    flex-basis: 100%;
    color: var(--text-muted);
  }
  .error {
    color: var(--danger);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
