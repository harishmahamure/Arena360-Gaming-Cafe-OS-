<script lang="ts">
  import { t } from "@arena/ui";
  import { skipProducts } from "./api";
  import SkipNote from "./SkipNote.svelte";
  import { STEP_NAMES } from "./wizard.svelte";
</script>

<h1 tabindex="-1">{t(STEP_NAMES.products)}</h1>
<p>{t("onboarding.products.intro")}</p>
<p>{t("onboarding.import_hint")}</p>
<SkipNote skip={skipProducts} note="onboarding.products.skip_note" />
