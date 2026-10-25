<script lang="ts">
  import { Button, TierLock, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { Snippet } from "svelte";
  import { isFree, refresh, shell } from "./shell.svelte";

  let { title, children }: { title: MessageKey; children?: Snippet } = $props();
</script>

<svelte:head><title>{t("app.title", { surface: t(title) })}</title></svelte:head>

<h1 class="type-web-display">{t(title)}</h1>
{#if !shell.ready}
  <p>{t("owner.loading")}</p>
{:else if !shell.branches}
  <p>{t("owner.unavailable")}</p>
  <Button label="common.retry" variant="secondary" onclick={refresh} />
{:else if isFree()}
  <TierLock tier="Basic" />
{:else if children}
  {@render children()}
{:else}
  <p>{t("owner.section.soon")}</p>
{/if}
