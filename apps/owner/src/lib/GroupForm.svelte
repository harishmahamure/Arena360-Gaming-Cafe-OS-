<script lang="ts">
  import { GroupChip, TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { propose } from "./commands.svelte";
  import {
    GROUP_COLOURS,
    GROUP_KINDS,
    blankGroup,
    groupFormOf,
    proposeGroup,
    validateGroup,
  } from "./devices";
  import type { BranchData, Listed } from "./pricing";
  import PricingForm from "./PricingForm.svelte";

  let {
    item,
    data,
    scope,
    oncancel,
  }: {
    /** The group being edited; none adds a group. */
    item: Listed | null;
    data: BranchData[];
    scope: string;
    oncancel: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let form = $state(item ? groupFormOf(item) : blankGroup());
  let errors: Record<string, MessageKey> = $state({});
  let el: HTMLFormElement | undefined = $state();

  const title = $derived(
    item ? t("owner.devices.group.edit", { name: item.name }) : t("owner.devices.group.form.add"),
  );

  function submit() {
    const checked = validateGroup(form);
    errors = checked.errors;
    if (!checked.ok) {
      void tick().then(() => focusFirstInvalid(el));
      return;
    }
    void propose(proposeGroup({ scope, data, origin: item?.key ?? null }, checked.values));
  }
</script>

<PricingForm {title} onsubmit={submit} {oncancel} bind:form={el}>
  <TextField label="owner.devices.group.name" bind:value={form.name} error={errors.name} required />
  <label class="field type-hub-label">
    <span>{t("owner.devices.group.kind")}</span>
    <select class="ui-interactive control" bind:value={form.kind}>
      {#each GROUP_KINDS as k (k.id)}<option value={k.id}>{t(k.label)}</option>{/each}
    </select>
  </label>
  <fieldset class="field type-hub-label">
    <legend>{t("owner.devices.group.colour")}</legend>
    <div class="inline">
      {#each GROUP_COLOURS as c (c.id)}
        <label class="check type-hub-label"
          ><input
            type="radio"
            name="group-colour"
            value={c.id}
            bind:group={form.colour}
          /><GroupChip name={t(c.label)} colour={c.id} /></label
        >
      {/each}
    </div>
  </fieldset>
</PricingForm>
