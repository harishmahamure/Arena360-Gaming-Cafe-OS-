<script lang="ts">
  import { TabControl } from "@arena/ui";
  import { onMount } from "svelte";
  import { TABS, read, write } from "../lib/pricing-state";

  /** Cafés · Partners under the title. The choice is `?tab=` in the URL and `data-pricing-tab`. */
  let selected = $state<string>(TABS[0]);

  onMount(() => {
    selected = read(location.search, "tab", TABS, TABS[0]);
    write("tab", selected); // the URL always names the tab
  });
</script>

<TabControl
  label="site.pricing.audience"
  items={[
    { id: "cafes", label: "site.pricing.tab.cafes" },
    { id: "partners", label: "site.pricing.tab.partners" },
  ]}
  bind:selected
  onchange={(id) => write("tab", id)}
/>
