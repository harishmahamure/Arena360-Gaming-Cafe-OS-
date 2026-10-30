<script lang="ts">
  import { Button, t } from "@arena/ui";

  let { branchName }: { branchName: string } = $props();
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.restore_pending.title") })}</title>
</svelte:head>

<section class="restore" aria-labelledby="restore-title" data-testid="hub-restore-pending">
  <h1 class="type-hub-numeric" id="restore-title">{t("hub.restore_pending.title")}</h1>
  <p class="type-hub-body" data-testid="restore-branch">
    {t("hub.restore_pending.branch")}: <strong>{branchName}</strong>
  </p>
  <p class="type-hub-body">{t("hub.restore_pending.replaces")}</p>
  <p class="type-hub-body">{t("hub.restore_pending.serves_nothing")}</p>
  <!-- The key is released by the owner in Story 9.10; until then the one option is off. -->
  <Button
    label="hub.restore_pending.option"
    disabled={true}
    disabledReason="hub.restore_pending.waiting"
  />
</section>

<style>
  .restore {
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
