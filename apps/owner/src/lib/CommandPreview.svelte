<script lang="ts">
  import { Button, Card, StatusBadge, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { cancel, choose, cmd, confirm, partsOf, whatOf } from "./commands.svelte";
  import { age } from "./shell.svelte";

  const REASONS: Record<string, MessageKey> = {
    FEATURE_NOT_ENTITLED: "owner.cmd.reason.entitled",
    BRANCH_UPDATE_REQUIRED: "owner.cmd.reason.update",
  };

  let heading: HTMLHeadingElement | undefined = $state();
  const ago = (asOf: string) => {
    const [key, n] = age(asOf);
    return t(key, { n });
  };

  const draft = $derived(cmd.draft);
  const targets = $derived(cmd.preview?.targets ?? []);
  const offered = $derived(targets.filter((x) => x.offered));
  // A change that differs per branch sends one part to each branch it fits, once the owner has
  // said what to do where the item is missing.
  const plan = $derived(draft ? partsOf(draft, targets) : { send: [], ask: [] });
  const sendable = $derived(draft?.variants ? plan.send.length : offered.length);
  const variantOf = (id: string) => draft?.variants?.find((v) => v.branch_id === id);
  const leftOut = (id: string) => draft?.excluded?.find((x) => x.branch_id === id);
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
        {#if cmd.known}<p class="note" role="status">{t("owner.cmd.preview.last_known")}</p>{/if}
        <h3 class="type-hub-label">{t("owner.cmd.preview.targets")}</h3>
        <ul class="targets">
          {#each targets as x (x.branch_id)}
            {@const seen = cmd.known?.[x.branch_id]}
            <li class="target" class:muted={!x.offered}>
              <span class="name">{x.name}</span>
              <StatusBadge
                tone={x.online ? "ok" : "muted"}
                label={cmd.known && !seen
                  ? "owner.cmd.preview.unknown"
                  : x.online
                    ? "owner.cmd.preview.online"
                    : "owner.cmd.preview.offline"}
              />
              {#if cmd.known}
                <span class="note type-hub-micro"
                  >{seen
                    ? t("owner.cmd.preview.last_known_age", { age: ago(seen) })
                    : t("owner.cmd.preview.never")}</span
                >
              {/if}
              {#if !x.offered}
                <span class="note type-hub-micro"
                  >{t("owner.cmd.preview.not_offered")} · {t(
                    REASONS[x.error?.code ?? ""] ?? "owner.cmd.reason.other",
                  )}</span
                >
              {:else if !x.online && !cmd.known}
                <span class="note type-hub-micro">{t("owner.cmd.preview.offline_note")}</span>
              {/if}
              {#if x.offered && leftOut(x.branch_id)}
                <span class="note type-hub-micro" data-excluded
                  >{t("owner.pricing.excluded.label", {
                    reason: leftOut(x.branch_id)?.reason ?? "",
                  })}</span
                >
              {:else if x.offered && variantOf(x.branch_id)?.ask}
                <div
                  class="ask"
                  role="group"
                  aria-label={t("owner.pricing.ask.group", { branch: x.name })}
                >
                  <span class="type-hub-micro note-inline">{t("owner.pricing.ask")}</span>
                  {#each ["create", "exclude"] as const as c (c)}
                    <label class="choice type-hub-label"
                      ><input
                        type="radio"
                        name="ask-{x.branch_id}"
                        checked={draft?.choices?.[x.branch_id] === c}
                        onchange={() => choose(x.branch_id, c)}
                      />{t(
                        c === "create" ? "owner.pricing.ask.create" : "owner.pricing.ask.exclude",
                      )}</label
                    >
                  {/each}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
        {#if !sendable && !plan.ask.length}<p>{t("owner.cmd.preview.none")}</p>
        {:else if cmd.known}<p class="note">{t("owner.cmd.preview.queue_note")}</p>{/if}
      {/if}
      {#if cmd.sendError}<p class="error" role="alert">{t(cmd.sendError)}</p>{/if}
      <div class="actions">
        <Button
          label="owner.cmd.send"
          onclick={confirm}
          disabled={cmd.previewing ||
            cmd.previewFailed ||
            cmd.sending ||
            !sendable ||
            plan.ask.length > 0}
          disabledReason={cmd.sending
            ? "owner.cmd.sending"
            : cmd.previewing
              ? "owner.cmd.preview.checking"
              : cmd.previewFailed
                ? "owner.cmd.preview.failed"
                : plan.ask.length
                  ? "owner.pricing.choose_first"
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
  .ask {
    flex-basis: 100%;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-4);
    min-width: 0;
  }
  .note-inline {
    color: var(--text-muted);
  }
  .choice {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
  }
  .choice input {
    inline-size: var(--space-touch-min);
    block-size: var(--space-touch-min);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
