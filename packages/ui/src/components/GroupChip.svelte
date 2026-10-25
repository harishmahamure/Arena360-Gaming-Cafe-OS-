<script lang="ts">
  let { name, colour }: { name: string; colour: string } = $props();
</script>

<span class="chip type-hub-micro" data-component="group-chip"
  ><span class="dot {colour}" aria-hidden="true"></span>{name}</span
>

<style>
  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    max-width: 100%;
    padding: var(--space-1) var(--space-2);
    border: var(--group-chip-border);
    border-radius: var(--radius-full);
    background-color: var(--group-chip-background);
    color: var(--group-chip-foreground);
    overflow-wrap: anywhere;
  }

  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: var(--radius-full);
  }

  .teal {
    background-color: var(--group-teal);
  }
  .orange {
    background-color: var(--group-orange);
  }
  .lime {
    background-color: var(--group-lime);
  }
  .sand {
    background-color: var(--group-sand);
  }
  .coral {
    background-color: var(--group-coral);
  }
  .silver {
    background-color: var(--group-silver);
  }
</style>
