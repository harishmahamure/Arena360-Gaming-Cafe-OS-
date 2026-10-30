<script lang="ts">
  import { TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { propose } from "./commands.svelte";
  import { sourceOf, type BranchData, type Listed } from "./pricing";
  import PricingForm from "./PricingForm.svelte";
  import { proposeThreshold, thresholdOf, validateThreshold } from "./products";

  let {
    item,
    data,
    scope,
    oncancel,
  }: { item: Listed; data: BranchData[]; scope: string; oncancel: () => void } = $props();

  // svelte-ignore state_referenced_locally
  const held = thresholdOf(sourceOf(item)?.row ?? item.entries[0].row);
  // svelte-ignore state_referenced_locally
  let level = $state(held === undefined ? "" : String(held));
  let error: MessageKey | undefined = $state();
  let el: HTMLFormElement | undefined = $state();

  function submit() {
    const checked = validateThreshold(level);
    error = "error" in checked ? checked.error : undefined;
    if ("error" in checked) {
      void tick().then(() => focusFirstInvalid(el));
      return;
    }
    void propose(proposeThreshold({ scope, data }, item.name, checked.value));
  }
</script>

<PricingForm
  title={t("owner.products.threshold_title", { name: item.name })}
  onsubmit={submit}
  {oncancel}
  bind:form={el}
>
  <TextField label="owner.products.threshold" bind:value={level} {error} inputmode="numeric" />
  <p class="hint type-hub-label">{t("hub.lowstock.help")}</p>
</PricingForm>
