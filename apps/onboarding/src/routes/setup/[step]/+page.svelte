<script lang="ts">
  import { t } from "@arena/ui";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import CafeStep from "../../../lib/CafeStep.svelte";
  import { STEP_NAMES, isStep, stepIndex, stepPath, wizard } from "../../../lib/wizard.svelte";

  const requested = $derived(page.params.step);
  const open = $derived.by(() => {
    const state = wizard.state;
    if (!state || !isStep(requested)) return null;
    const done = state.steps.some((s) => s.key === requested && s.state === "done");
    return done || stepIndex(requested) <= stepIndex(state.current) ? requested : null;
  });

  $effect(() => {
    const state = wizard.state;
    if (state && open === null) goto(stepPath(state.current), { replaceState: true });
  });
</script>

{#if open === "cafe"}
  <CafeStep />
{:else if open}
  <h1 tabindex="-1">{t(STEP_NAMES[open])}</h1>
  <p class="soon">{t("onboarding.setup.coming_soon")}</p>
{/if}

<style>
  .soon {
    margin: 0;
    line-height: var(--type-hub-label-line-height);
  }
</style>
