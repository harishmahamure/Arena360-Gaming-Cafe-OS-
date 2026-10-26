<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import DevicesStep from "../../../lib/DevicesStep.svelte";
  import CafeStep from "../../../lib/CafeStep.svelte";
  import GoLiveStep from "../../../lib/GoLiveStep.svelte";
  import InstallHubStep from "../../../lib/InstallHubStep.svelte";
  import PrinterStep from "../../../lib/PrinterStep.svelte";
  import ProductsStep from "../../../lib/ProductsStep.svelte";
  import RatesStep from "../../../lib/RatesStep.svelte";
  import StaffStep from "../../../lib/StaffStep.svelte";
  import TestSessionStep from "../../../lib/TestSessionStep.svelte";
  import TierStep from "../../../lib/TierStep.svelte";
  import { isStep, stepIndex, stepPath, wizard } from "../../../lib/wizard.svelte";

  const requested = $derived(page.params.step);
  const open = $derived.by(() => {
    const state = wizard.state;
    if (!state || !isStep(requested)) return null;
    const done = state.steps.some((s) => s.key === requested && s.state !== "pending");
    return done || stepIndex(requested) <= stepIndex(state.current) ? requested : null;
  });

  $effect(() => {
    const state = wizard.state;
    if (state && open === null) goto(stepPath(state.current), { replaceState: true });
  });
</script>

{#if open === "cafe"}
  <CafeStep />
{:else if open === "tier"}
  <TierStep />
{:else if open === "install_hub"}
  <InstallHubStep />
{:else if open === "devices"}
  <DevicesStep />
{:else if open === "rates"}
  <RatesStep />
{:else if open === "products"}
  <ProductsStep />
{:else if open === "staff"}
  <StaffStep />
{:else if open === "printer_payments"}
  <PrinterStep />
{:else if open === "test_session"}
  <TestSessionStep />
{:else if open === "go_live"}
  <GoLiveStep />
{/if}
