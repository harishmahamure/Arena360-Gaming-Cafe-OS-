<script lang="ts">
  import { Button, TierLock, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { Snippet } from "svelte";
  import { isFree, refresh, shell } from "./shell.svelte";

  let { title, children }: { title: MessageKey; children?: Snippet } = $props();
  const mixed = $derived(shell.branches?.some((b) => b.tier === "free"));
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
{:else if isFree()}
  <TierLock tier="Basic" />
{:else if mixed}
  <ul class="branches">
    {#each shell.branches as b (b.id)}
      <li>
        <h2 class="type-hub-label">{b.name}</h2>
        {#if b.tier === "free"}<TierLock tier="Basic" />{:else}{@render body()}{/if}
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
