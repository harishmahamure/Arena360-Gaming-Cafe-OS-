<script lang="ts">
  import { Button, TabControl, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import CatalogueScope from "./CatalogueScope.svelte";
  import CouponsPanel from "./CouponsPanel.svelte";
  import RatesPlansPanel from "./RatesPlansPanel.svelte";
  import RulesPanel from "./RulesPanel.svelte";

  const TABS = [
    { id: "plans", label: "owner.pricing.tab.plans" },
    { id: "rules", label: "owner.pricing.tab.rules" },
    { id: "coupons", label: "owner.pricing.tab.coupons" },
  ] as const satisfies readonly { id: string; label: MessageKey }[];
  type TabId = (typeof TABS)[number]["id"];

  const fromUrl = (): TabId => {
    const wanted = page.url.searchParams.get("tab");
    return TABS.find((x) => x.id === wanted)?.id ?? "plans";
  };
  let tab: string = $state(fromUrl());
  $effect(() => {
    tab = fromUrl();
  });

  // The selected tab is in the URL (`?tab=`), without adding history entries.
  function select(id: string) {
    const url = new URL(page.url.href);
    url.searchParams.set("tab", id);
    void goto(url, { replace: true, reset: false });
  }

  // Opened without a (valid) `?tab=`: say which tab it is.
  onMount(() => {
    if (page.url.searchParams.get("tab") !== tab) select(tab);
  });
</script>

<CatalogueScope label="owner.pricing.tabs" component="pricing-board">
  {#snippet children({ data, scope, now, failed, waiting, retry })}
    <TabControl
      items={[...TABS]}
      label="owner.pricing.tabs"
      bind:selected={tab}
      onchange={select}
    />
    <div
      role="tabpanel"
      aria-label={t(TABS.find((x) => x.id === tab)?.label ?? "owner.pricing.tab.plans")}
      class="panels"
    >
      {#if failed}
        <p>{t("owner.unavailable")}</p>
        <Button label="common.retry" variant="secondary" onclick={retry} />
      {:else if waiting && !data.some((d) => d.cat)}
        <p>{t("owner.pricing.loading")}</p>
      {:else if tab === "rules"}
        <RulesPanel {data} {scope} {now} />
      {:else if tab === "coupons"}
        <CouponsPanel {data} {scope} />
      {:else}
        <RatesPlansPanel {data} {scope} />
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
