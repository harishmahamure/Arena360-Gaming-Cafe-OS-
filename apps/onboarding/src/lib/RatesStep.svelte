<script lang="ts">
  import { t } from "@arena/ui";
  import HubAge from "./HubAge.svelte";
  import { watchSetup } from "./setupView.svelte";
  import { STEP_NAMES } from "./wizard.svelte";

  const w = watchSetup();
  const missing = $derived(w.view?.setup?.groups.filter((g) => !g.has_rate) ?? []);
</script>

<h1 tabindex="-1">{t(STEP_NAMES.rates)}</h1>
<p>{t("onboarding.rates.intro")}</p>
<HubAge view={w.view} />
{#if missing.length}
  <h2>{t("onboarding.rates.missing")}</h2>
  <ul data-testid="rates-missing">
    {#each missing as g (g.id)}<li>{g.name}</li>{/each}
  </ul>
{/if}
<p>{t("onboarding.import_hint")}</p>
