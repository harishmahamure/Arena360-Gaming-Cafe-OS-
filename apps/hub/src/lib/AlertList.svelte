<script lang="ts">
  import { t } from "@arena/ui";
  import { onMount } from "svelte";
  import { ageOf, type AlertRow } from "./alerts.svelte";
  import { pushBack } from "./back";

  let {
    id,
    rows,
    names,
    onpick,
    onclose,
  }: {
    id: string;
    rows: AlertRow[];
    names: Record<string, string>;
    onpick: (row: AlertRow) => void;
    onclose: () => void;
  } = $props();

  let now = $state(Date.now());
  let list: HTMLUListElement | undefined = $state();
  let section: HTMLElement | undefined = $state();

  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 15_000);
    (list?.querySelector<HTMLElement>("button") ?? section)?.focus();
    // Android Back closes the list like Esc does.
    const unback = pushBack(() => onclose());
    return () => {
      clearInterval(timer);
      unback();
    };
  });

  const where = (row: AlertRow) =>
    row.deviceId ? (names[row.deviceId] ?? "") : t("hub.alerts.branch");
  const acts = (row: AlertRow) => row.alertId !== null;
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section
  {id}
  class="panel"
  bind:this={section}
  tabindex="-1"
  aria-labelledby="{id}-title"
  data-testid="alert-list"
  onkeydown={(e) => e.key === "Escape" && onclose()}
>
  <h2 class="type-hub-title" id="{id}-title">{t("hub.alerts.title")}</h2>
  <ul bind:this={list}>
    {#each rows as row (row.key)}
      <li data-testid="alert-row" data-alert-id={row.alertId ?? undefined}>
        {#if acts(row)}
          <button type="button" class="row ui-interactive" onclick={() => onpick(row)}>
            <span class="type-hub-title">{where(row)}</span>
            <span class="type-hub-body">{t(row.label)}</span>
            {#if row.since !== null}
              <span class="type-hub-label muted">{ageOf(row.since, now)}</span>
            {/if}
          </button>
        {:else}
          <p class="row">
            <span class="glyph" aria-hidden="true">⚠</span>
            <span class="type-hub-title">{where(row)}</span>
            <span class="type-hub-body">{t(row.label)}</span>
          </p>
        {/if}
      </li>
    {/each}
  </ul>
</section>

<style>
  .panel {
    position: absolute;
    inset-block-start: 100%;
    inset-inline-end: var(--space-hub-gutter);
    z-index: 6;
    box-sizing: border-box;
    width: min(28rem, calc(100vw - 2 * var(--space-3)));
    max-height: 70dvh;
    overflow-y: auto;
    padding: var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--hub-header-background);
    color: var(--text);
  }

  /* Phone: the list sits inside the screen edges, under the 64px header. */
  @media (max-width: 839.98px) {
    .panel {
      inset-inline-end: var(--space-3);
    }
  }

  h2,
  p,
  ul {
    margin: 0;
  }

  ul {
    display: grid;
    gap: var(--space-2);
    padding: 0;
    margin-block-start: var(--space-3);
    list-style: none;
  }

  .row {
    --ui-bg: transparent;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-2) var(--space-3);
    box-sizing: border-box;
    width: 100%;
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: pointer;
  }

  p.row {
    cursor: default;
    align-items: center;
  }

  .muted {
    color: var(--text-muted);
  }

  .glyph {
    color: var(--warning);
  }
</style>
