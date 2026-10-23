<script lang="ts">
  import type { Snippet } from "svelte";
  import { t, type MessageKey } from "../messages/index.js";
  import type { ForceState } from "./types.js";

  type Item = { id: string; label: MessageKey };

  /**
   * ARIA tabs with roving tabindex: Arrow keys (wrapping), Home and End move and select.
   * `onchange` reports the newly selected id; syncing it to the URL is the caller's job.
   */
  let {
    items,
    label,
    selected = $bindable(items[0]?.id),
    onchange,
    forceState,
    panel,
  }: {
    items: Item[];
    label: MessageKey;
    selected?: string;
    onchange?: (id: string) => void;
    /** Gallery only: forced state on the selected tab. */
    forceState?: ForceState;
    panel?: Snippet<[string]>;
  } = $props();

  const id = $props.id();
  const tabId = (item: string) => id + "-tab-" + item;
  const panelId = id + "-panel";

  let tabs: HTMLButtonElement[] = $state([]);

  // A `selected` id that is not among the items falls back to the first item, so one tab
  // is always tabbable.
  const current = $derived(items.some((item) => item.id === selected) ? selected : items[0]?.id);

  function select(index: number, focus: boolean) {
    const item = items[index];
    if (!item) return;
    if (item.id !== current) {
      selected = item.id;
      onchange?.(item.id);
    }
    if (focus) tabs[index]?.focus();
  }

  function onkeydown(event: KeyboardEvent, index: number) {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const last = items.length - 1;
    const next =
      event.key === "ArrowRight"
        ? index === last
          ? 0
          : index + 1
        : event.key === "ArrowLeft"
          ? index === 0
            ? last
            : index - 1
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : -1;
    if (next === -1) return;
    event.preventDefault();
    select(next, true);
  }
</script>

<div class="tabs" data-component="tab-control">
  <div class="track" role="tablist" aria-label={t(label)}>
    {#each items as item, index (item.id)}
      {@const isSelected = item.id === current}
      <button
        bind:this={tabs[index]}
        type="button"
        role="tab"
        class="ui-interactive tab"
        class:selected={isSelected}
        id={tabId(item.id)}
        aria-selected={isSelected}
        aria-controls={panel ? panelId : undefined}
        tabindex={isSelected ? 0 : -1}
        data-force-state={isSelected ? forceState : undefined}
        onclick={() => select(index, false)}
        onkeydown={(event) => onkeydown(event, index)}>{t(item.label)}</button
      >
    {/each}
  </div>
  {#if panel && current}
    <div class="panel" role="tabpanel" id={panelId} aria-labelledby={tabId(current)} tabindex="0">
      {@render panel(current)}
    </div>
  {/if}
</div>

<style>
  .tabs {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-3);
    max-width: 100%;
  }

  .track {
    --ring-gap: var(--tab-control-background);
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    max-width: 100%;
    box-sizing: border-box;
    padding: var(--space-1);
    border: 1px solid var(--line);
    border-radius: var(--radius-full);
    background-color: var(--tab-control-background);
  }

  .tab {
    --ui-bg: var(--tab-control-background);
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    padding: 0 var(--space-5);
    border: 0;
    border-radius: var(--radius-full);
    color: var(--text-muted);
    font: inherit;
    font-size: var(--type-hub-body-size);
    font-weight: var(--type-hub-title-weight);
    cursor: pointer;
  }

  .tab.selected {
    --ui-bg: var(--text);
    color: var(--bg);
  }

  .panel {
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    box-sizing: border-box;
    display: flex;
    align-items: center;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
  }
</style>
