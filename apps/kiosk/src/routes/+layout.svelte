<script lang="ts">
  import "@arena/ui/base.css";
  import { page } from "$app/state";
  import type { Snippet } from "svelte";
  import KioskStage from "../lib/KioskStage.svelte";

  let { children }: { children: Snippet } = $props();

  /** The overlay HUD is its own window (Story 6.14): none of the kiosk stage wraps it. */
  const hud = $derived(page.url.pathname === "/hud");
</script>

{#if hud}
  {@render children()}
{:else}
  <KioskStage>{@render children()}</KioskStage>
{/if}
