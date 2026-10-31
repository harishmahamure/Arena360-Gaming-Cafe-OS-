<script lang="ts">
  import { TabControl } from "@arena/ui";
  import { onMount } from "svelte";
  import { CYCLES, read, write, type Cycle } from "../lib/pricing-state";

  /**
   * Monthly · Yearly above the plan cards. The choice is `?billing=` in the URL and
   * `data-pricing-billing`; the prices of both cycles are in the page and the stylesheet shows one.
   * Each change is announced to screen readers with the new prices (`announce`, ready-made text).
   */
  let { announce }: { announce: Record<Cycle, string> } = $props();

  let selected = $state<string>(CYCLES[0]);
  let status = $state("");

  const choose = (id: string) => {
    write("billing", id);
    status = announce[id as Cycle];
  };

  onMount(() => {
    selected = read(location.search, "billing", CYCLES, CYCLES[0]);
    write("billing", selected);
  });
</script>

<TabControl
  label="site.pricing.billing"
  items={[
    { id: "monthly", label: "site.pricing.billing.monthly" },
    { id: "yearly", label: "site.pricing.billing.yearly" },
  ]}
  bind:selected
  onchange={choose}
/>
<p class="visually-hidden" role="status" aria-live="polite">{status}</p>
