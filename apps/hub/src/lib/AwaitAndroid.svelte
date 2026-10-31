<script lang="ts">
  import { t } from "@arena/ui";

  /** The Windows Hub of a Hub move, paired with the owner's code and waiting for the Android Hub. */
  let { branchName }: { branchName: string } = $props();
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.migration_wait.title") })}</title>
</svelte:head>

<section class="wait" aria-labelledby="wait-title" data-testid="hub-migration-wait">
  <h1 class="type-hub-numeric" id="wait-title">{t("hub.migration_wait.title")}</h1>
  <p class="type-hub-body" data-testid="wait-branch">
    {t("hub.migration_wait.branch")}: <strong>{branchName}</strong>
  </p>
  <p class="type-hub-body">{t("hub.migration_wait.steps")}</p>
  <p class="type-hub-body">{t("hub.migration_wait.serves_nothing")}</p>
</section>

<style>
  .wait {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 60ch;
  }

  h1,
  p {
    margin: 0;
  }
</style>
