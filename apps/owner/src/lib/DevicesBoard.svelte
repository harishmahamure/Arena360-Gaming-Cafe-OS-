<script lang="ts">
  import { Button, t } from "@arena/ui";
  import CatalogueScope from "./CatalogueScope.svelte";
  import DevicesPanel from "./DevicesPanel.svelte";
</script>

<CatalogueScope label="owner.manage.area.devices" component="devices-board">
  {#snippet children({ data, scope, failed, waiting, retry })}
    <div class="panels">
      {#if failed}
        <p>{t("owner.unavailable")}</p>
        <Button label="common.retry" variant="secondary" onclick={retry} />
      {:else if waiting && !data.some((d) => d.cat)}
        <p>{t("owner.devices.loading")}</p>
      {:else}
        <DevicesPanel {data} {scope} />
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
