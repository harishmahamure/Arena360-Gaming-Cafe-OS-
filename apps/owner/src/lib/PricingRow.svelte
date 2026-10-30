<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * One item of a Manage → Pricing list: what it is, and an Edit button when the branches sent
   * the fields to edit it with (an older summary lists the name only).
   */
  let {
    kind,
    name,
    edit,
    children,
  }: {
    kind: string;
    name: string;
    /** The button's text (for example "Edit Night pass"); none hides it. */
    edit?: { label: string; onclick: () => void };
    children: Snippet;
  } = $props();
</script>

<li class="row" data-component="pricing-row" data-kind={kind} data-name={name}>
  <div class="what">{@render children()}</div>
  {#if edit}
    <button type="button" class="ui-interactive btn type-hub-label" onclick={edit.onclick}
      >{edit.label}</button
    >
  {/if}
</li>

<style>
  .row {
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
    overflow-wrap: anywhere;
  }
  .what :global(p),
  .what :global(ul) {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .what :global(.muted) {
    color: var(--text-muted);
  }
  .btn {
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--surface-2);
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
</style>
