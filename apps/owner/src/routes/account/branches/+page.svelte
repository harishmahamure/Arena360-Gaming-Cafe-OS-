<script lang="ts">
  import { Button, t } from "@arena/ui";
  import { refresh, shell } from "../../../lib/shell.svelte";
</script>

<svelte:head><title>{t("app.title", { surface: t("owner.account.branches") })}</title></svelte:head>

<h1 class="type-web-display">{t("owner.account.branches")}</h1>
{#if shell.branches?.length}
  <ul>
    {#each shell.branches as b (b.id)}<li>{b.name}</li>{/each}
  </ul>
{:else if shell.ready}
  <p>{shell.branches ? t("owner.branches.empty") : t("owner.unavailable")}</p>
  {#if !shell.branches}<Button label="common.retry" variant="secondary" onclick={refresh} />{/if}
{/if}
<a href="/account">{t("owner.account.back")}</a>
