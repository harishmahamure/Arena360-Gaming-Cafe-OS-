<script lang="ts">
  import { TabControl, TierLock, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import CommandList from "./CommandList.svelte";
  import CommandPreview from "./CommandPreview.svelte";
  import { PROPOSE_EVENT, cancel, propose, type CommandDraft } from "./commands.svelte";
  import PricingBoard from "./PricingBoard.svelte";
  import ProductsBoard from "./ProductsBoard.svelte";
  import SetupCodes from "./SetupCodes.svelte";
  import SetupGate from "./SetupGate.svelte";
  import StaffBoard from "./StaffBoard.svelte";
  import { isLive, shell } from "./shell.svelte";

  // A branch that is in setup or on Free can't take a change: it says so, beside the one list.
  const gated = $derived((shell.branches ?? []).filter((b) => !isLive(b) || b.tier === "free"));

  const AREAS = [
    { id: "pricing", label: "owner.manage.area.pricing" },
    { id: "products", label: "owner.manage.area.products" },
    { id: "staff", label: "owner.manage.area.staff" },
  ] as const satisfies readonly { id: string; label: MessageKey }[];

  // The area is in the URL (`?area=`, default pricing), without adding history entries. The
  // Pricing tab belongs to Pricing alone, so leaving it drops `?tab=`.
  const fromUrl = () =>
    AREAS.find((a) => a.id === page.url.searchParams.get("area"))?.id ?? "pricing";
  let area: string = $state(fromUrl());
  $effect(() => {
    area = fromUrl();
  });
  function select(id: string) {
    const url = new URL(page.url.href);
    url.searchParams.set("area", id);
    url.searchParams.delete("tab");
    void goto(url, { replace: true, reset: false });
  }

  // The Manage forms (Pricing, Story 8.23) call `propose` themselves; the event lets any other
  // screen open the preview the same way.
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
  <TabControl
    items={[...AREAS]}
    label="owner.manage.areas"
    bind:selected={area}
    onchange={select}
  />
  <div
    role="tabpanel"
    aria-label={t(AREAS.find((a) => a.id === area)?.label ?? "owner.manage.area.pricing")}
  >
    {#if area === "products"}
      <ProductsBoard />
    {:else if area === "staff"}
      <StaffBoard />
    {:else}
      <PricingBoard />
    {/if}
  </div>
  <SetupCodes />
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
