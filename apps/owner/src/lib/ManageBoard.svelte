<script lang="ts">
  import { TierLock } from "@arena/ui";
  import { onMount } from "svelte";
  import CommandList from "./CommandList.svelte";
  import CommandPreview from "./CommandPreview.svelte";
  import { PROPOSE_EVENT, cancel, propose, type CommandDraft } from "./commands.svelte";
  import SetupGate from "./SetupGate.svelte";
  import { isLive, shell } from "./shell.svelte";

  // A branch that is in setup or on Free can't take a change: it says so, beside the one list.
  const gated = $derived((shell.branches ?? []).filter((b) => !isLive(b) || b.tier === "free"));

  // The Manage forms (Story 8.23 on) call `propose` themselves; until one exists this event is how
  // a change reaches the preview.
  onMount(() => {
    const open = (e: Event) => void propose((e as CustomEvent<CommandDraft>).detail);
    addEventListener(PROPOSE_EVENT, open);
    return () => {
      removeEventListener(PROPOSE_EVENT, open);
      cancel();
    };
  });
</script>

<div class="board">
  {#if gated.length}
    <ul class="gates">
      {#each gated as b (b.id)}
        <li>
          <h2 class="type-hub-label">{b.name}</h2>
          {#if !isLive(b)}
            <SetupGate branch={b} />
          {:else}
            <TierLock tier="Basic" />
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  <CommandPreview />
  <CommandList />
</div>

<style>
  .board {
    display: grid;
    gap: var(--space-5);
  }
  .gates {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-4);
  }
  h2 {
    margin: 0 0 var(--space-2);
  }
</style>
