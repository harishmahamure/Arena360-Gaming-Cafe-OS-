<script lang="ts">
  import { TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { propose } from "./commands.svelte";
  import {
    exponentOf,
    norm,
    proposeRate,
    rateFormOf,
    validateRate,
    type BranchData,
    type RateLine,
  } from "./pricing";
  import PricingForm from "./PricingForm.svelte";

  let {
    line,
    data,
    scope,
    oncancel,
  }: { line: RateLine; data: BranchData[]; scope: string; oncancel: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let form = $state(rateFormOf(line));
  let errors: Record<string, MessageKey> = $state({});
  let el: HTMLFormElement | undefined = $state();

  function submit() {
    const checked = validateRate(form, exponentOf(data));
    errors = checked.errors;
    if (!checked.ok) {
      void tick().then(() => focusFirstInvalid(el));
      return;
    }
    void propose(proposeRate({ scope, data, origin: norm(line.name) }, form));
  }
</script>

<PricingForm
  title={t("hub.groups.rate_edit", { name: line.name })}
  onsubmit={submit}
  {oncancel}
  bind:form={el}
>
  <TextField
    label="hub.groups.rate"
    bind:value={form.price}
    error={errors.price}
    inputmode="decimal"
    required
  />
</PricingForm>
