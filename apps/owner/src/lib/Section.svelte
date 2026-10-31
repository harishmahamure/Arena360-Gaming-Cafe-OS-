<script lang="ts">
  import { Button, TierLock, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { Snippet } from "svelte";
  import SetupGate from "./SetupGate.svelte";
  import { isFree, isLive, refresh, setupBranches, shell } from "./shell.svelte";

  /** `all`: the section draws its own per-branch entries (Today), so paid tenants get `children` once. */
  let {
    title,
    scope = "branch",
    children,
  }: { title: MessageKey; scope?: "branch" | "all"; children?: Snippet } = $props();
  // Free limits apply once a branch is live; until then it is in setup mode.
  const locked = $derived(isFree() && !setupBranches().length);
  const mixed = $derived(shell.branches?.some((b) => b.tier === "free" || !isLive(b)));
</script>

<svelte:head><title>{t("app.title", { surface: t(title) })}</title></svelte:head>

<h1 class="type-web-display">{t(title)}</h1>
{#snippet body()}
  {#if children}
    {@render children()}
  {:else}
    <p>{t("owner.section.soon")}</p>
  {/if}
{/snippet}
{#if !shell.ready}
  <p>{t("owner.loading")}</p>
{:else if !shell.branches}
  <p>{t("owner.unavailable")}</p>
  <Button label="common.retry" variant="secondary" onclick={refresh} />
{:else if locked}
  <TierLock tier="Basic" branch={shell.branches.length === 1 ? shell.branches[0]?.id : undefined} />
{:else if mixed && scope === "branch"}
  <ul class="branches">
    {#each shell.branches as b (b.id)}
      <li>
        <h2 class="type-hub-label">{b.name}</h2>
        {#if !isLive(b)}
          <SetupGate branch={b} />
        {:else if b.tier === "free"}
          <TierLock tier="Basic" branch={b.id} />
        {:else}
          {@render body()}
        {/if}
      </li>
    {/each}
  </ul>
{:else}
  {@render body()}
{/if}

<style>
  .branches {
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
