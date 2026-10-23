<script lang="ts">
  import { Button, formatDateTime, t } from "@arena/ui";
  import type { AuditDetail } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { actorLabel, opLabel } from "./labels";

  /**
   * One change, read only: who, when, what, the approver of an elevated change, and the row's
   * before and after images side by side. A changed field carries the visible "Changed" marker
   * (never colour alone); a created row says "Created" in the before column; secrets read
   * "Hidden". Esc or Close closes it.
   */
  let {
    detail,
    tz,
    onclose,
  }: { detail: AuditDetail; tz: string | null | undefined; onclose: () => void } = $props();

  const REDACTED = "[redacted]"; // i18n-ignore (the Hub's redaction marker, never shown)
  const MODES: Record<string, MessageKey> = {
    hub: "hub.audit.mode.hub",
    cached_verifier: "hub.audit.mode.cached_verifier",
    unknown: "hub.audit.mode.unknown",
  };

  let heading: HTMLHeadingElement | undefined = $state();
  let panel: HTMLElement | undefined = $state();

  const fields = $derived.by(() => {
    const keys = new Set([...Object.keys(detail.before ?? {}), ...Object.keys(detail.after)]);
    return [...keys].sort();
  });
  const changed = $derived(new Set(detail.changed));
  const created = $derived(detail.item.op === "insert");
  const deleted = $derived(detail.item.op === "delete");

  function show(value: string | number | null | undefined): string {
    if (value === null || value === undefined) return "—"; // i18n-ignore (an empty value)
    if (value === REDACTED) return t("hub.audit.detail.redacted");
    return String(value);
  }

  onMount(() => heading?.focus());
</script>

<svelte:window
  onkeydown={(e) => {
    // Esc closes the details while focus is inside them.
    if (e.key === "Escape" && panel?.contains(document.activeElement)) {
      e.preventDefault();
      onclose();
    }
  }}
/>

<section
  class="detail"
  aria-labelledby="audit-detail-title"
  data-testid="audit-detail"
  bind:this={panel}
>
  <div class="head">
    <h2 class="type-hub-title" id="audit-detail-title" tabindex="-1" bind:this={heading}>
      {t("hub.audit.detail.title")}
    </h2>
    <Button label="hub.audit.detail.close" variant="secondary" onclick={onclose} />
  </div>
  <dl class="meta type-hub-label">
    <dt>{t("hub.audit.col.time")}</dt>
    <dd class="tabular-nums">{formatDateTime(detail.item.at, tz)}</dd>
    <dt>{t("hub.audit.col.actor")}</dt>
    <dd>{actorLabel(detail.item.actor)}</dd>
    <dt>{t("hub.audit.col.intent")}</dt>
    <dd>{detail.item.intent_kind} · {t(opLabel(detail.item.op))}</dd>
    <dt>{t("hub.audit.col.record")}</dt>
    <dd>{detail.item.tbl} / {detail.item.row_id}</dd>
  </dl>
  {#if detail.item.elevated_by}
    <p class="approved type-hub-label" data-testid="audit-approved-by">
      {t("hub.audit.detail.approved_by", {
        name: detail.item.elevated_by.name,
        mode: MODES[detail.item.elevated_by.mode]
          ? t(MODES[detail.item.elevated_by.mode])
          : detail.item.elevated_by.mode,
      })}
    </p>
  {/if}
  {#if deleted}
    <p class="deleted type-hub-label" data-testid="audit-deleted">
      <strong>{t("hub.audit.detail.deleted")}</strong>
      <span>{t("hub.audit.detail.deleted_note")}</span>
    </p>
  {/if}
  <div class="scroll">
    <table class="images type-hub-label">
      <thead>
        <tr>
          <th scope="col">{t("hub.audit.detail.field")}</th>
          <th scope="col">{t("hub.audit.detail.before")}</th>
          <th scope="col"
            >{t("hub.audit.detail.after")}{#if deleted}
              · {t("hub.audit.detail.deleted")}{/if}</th
          >
          <th scope="col">{t("hub.audit.detail.changed")}</th>
        </tr>
      </thead>
      <tbody>
        {#each fields as field, i (field)}
          <tr class:changed={changed.has(field)}>
            <th scope="row" class="field">{field}</th>
            {#if created}
              {#if i === 0}
                <td class="created" rowspan={fields.length} data-testid="audit-created"
                  >{t("hub.audit.detail.created")}</td
                >
              {/if}
            {:else}
              <td class="value">{show(detail.before?.[field])}</td>
            {/if}
            <td class="value">{show(detail.after[field])}</td>
            {#if changed.has(field)}
              <td
                class="marker"
                aria-label={t("hub.audit.detail.changed_label", { field })}
                data-testid="audit-changed">{t("hub.audit.detail.changed")}</td
              >
            {:else}
              <td></td>
            {/if}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</section>

<style>
  .detail {
    display: grid;
    gap: var(--space-3);
    align-content: start;
    padding: var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    min-width: 0;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  h2,
  .meta,
  .approved,
  .deleted {
    margin: 0;
  }

  .deleted {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 2px solid var(--border-strong);
    border-radius: var(--radius-sm);
  }

  .meta {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-1) var(--space-3);
  }

  .meta dt {
    color: var(--text-muted);
  }

  .meta dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .approved {
    font-weight: var(--type-hub-title-weight);
  }

  .scroll {
    overflow-x: auto;
  }

  .images {
    width: 100%;
    border-collapse: collapse;
  }

  .images th,
  .images td {
    padding: var(--space-2);
    border-bottom: 1px solid var(--border-strong);
    text-align: left;
    vertical-align: top;
    overflow-wrap: anywhere;
  }

  .images thead th {
    color: var(--text-muted);
  }

  .field {
    font-weight: var(--type-hub-title-weight);
  }

  .created {
    color: var(--text-muted);
    font-style: italic;
  }

  /* A changed field: the "Changed" text marks it; the heavier weight is extra. */
  .changed .value {
    font-weight: var(--type-hub-title-weight);
  }

  .marker {
    white-space: nowrap;
    font-weight: var(--type-hub-title-weight);
  }
</style>
