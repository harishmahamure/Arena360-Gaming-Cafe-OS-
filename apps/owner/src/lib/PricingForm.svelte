<script lang="ts">
  import { Button, Card } from "@arena/ui";
  import { onMount, type Snippet } from "svelte";

  /**
   * The frame of a Manage → Pricing form: a title, the fields, and Send / Cancel. Send checks the
   * fields (the caller's `onsubmit`) and opens the preview; nothing is sent from here.
   */
  let {
    title,
    onsubmit,
    oncancel,
    children,
    form = $bindable(),
  }: {
    title: string;
    onsubmit: () => void;
    oncancel: () => void;
    children: Snippet;
    form?: HTMLFormElement;
  } = $props();

  // The form opens under the row that asked for it: move the reader to its first field.
  onMount(() => form?.querySelector<HTMLElement>("input:not(:disabled)")?.focus());
</script>

<Card>
  <form
    class="pform"
    bind:this={form}
    aria-label={title}
    data-component="pricing-form"
    novalidate
    onsubmit={(e) => {
      e.preventDefault();
      onsubmit();
    }}
  >
    <h3 class="type-hub-title">{title}</h3>
    {@render children()}
    <div class="actions">
      <Button label="owner.pricing.form.send" type="submit" />
      <Button label="owner.pricing.form.cancel" variant="secondary" onclick={oncancel} />
    </div>
  </form>
</Card>

<style>
  .pform {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }
  h3 {
    margin: 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .pform :global(fieldset) {
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }
  .pform :global(.field) {
    display: grid;
    gap: var(--space-1);
    min-width: 0;
  }
  .pform :global(.inline) {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .pform :global(.check) {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
  }
  .pform :global(.check input) {
    inline-size: var(--space-touch-min);
    block-size: var(--space-touch-min);
  }
  .pform :global(.control) {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
    font: inherit;
  }
  .pform :global(.hint) {
    margin: 0;
    color: var(--text-muted);
  }
  .pform :global(.error) {
    margin: 0;
    color: var(--danger);
  }
</style>
