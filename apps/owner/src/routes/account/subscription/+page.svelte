<script lang="ts">
  import { Button, Card, t } from "@arena/ui";
  import { refresh, shell } from "../../../lib/shell.svelte";

  const TIER = {
    free: "owner.tier.free",
    basic: "owner.tier.basic",
    premium: "owner.tier.premium",
  } as const;
</script>

<svelte:head
  ><title>{t("app.title", { surface: t("owner.account.subscription") })}</title></svelte:head
>

<h1 class="type-web-display">{t("owner.account.subscription")}</h1>
{#if shell.branches}
  <ul class="list">
    {#each shell.branches as b (b.id)}
      <li>
        <Card>
          <h2 class="type-hub-title">{b.name}</h2>
          <p>{t("owner.subscription.tier")}: {t(TIER[b.tier])}</p>
          <p>
            {t("owner.subscription.flags")}: {b.flags.length
              ? b.flags.join(", ")
              : t("owner.subscription.no_flags")}
          </p>
        </Card>
      </li>
    {/each}
  </ul>
{:else if shell.ready}
  <p>{t("owner.unavailable")}</p>
  <Button label="common.retry" variant="secondary" onclick={refresh} />
{/if}
<p>{t("owner.subscription.soon")}</p>
<a href="/account">{t("owner.account.back")}</a>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: 0;
    list-style: none;
  }
</style>
