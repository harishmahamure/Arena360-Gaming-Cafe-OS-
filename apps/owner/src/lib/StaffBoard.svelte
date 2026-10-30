<script lang="ts">
  import { Button, t } from "@arena/ui";
  import CatalogueScope from "./CatalogueScope.svelte";
  import StaffPanel from "./StaffPanel.svelte";
</script>

<CatalogueScope label="owner.manage.area.staff" component="staff-board">
  {#snippet children({ data, scope, failed, waiting, retry })}
    <div class="panels">
      {#if failed}
        <p>{t("owner.unavailable")}</p>
        <Button label="common.retry" variant="secondary" onclick={retry} />
      {:else if waiting && !data.some((d) => d.cat)}
        <p>{t("owner.staff.loading")}</p>
      {:else}
        <StaffPanel {data} {scope} />
      {/if}
    </div>
  {/snippet}
</CatalogueScope>

<style>
  .panels {
    display: grid;
    gap: var(--space-5);
    min-width: 0;
  }
  p {
    margin: 0;
  }
</style>
