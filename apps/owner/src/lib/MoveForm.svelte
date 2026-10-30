<script lang="ts">
  import { t } from "@arena/ui";
  import { propose } from "./commands.svelte";
  import { groupsFor, proposeAssign, type DeviceLine } from "./devices";
  import PricingForm from "./PricingForm.svelte";

  let { line, oncancel }: { line: DeviceLine; oncancel: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let group = $state(line.device.group_id ?? "");
  const options = $derived(groupsFor(line));

  const submit = () => {
    if (group === (line.device.group_id ?? "")) return oncancel();
    void propose(proposeAssign(line, group === "" ? null : group));
  };
</script>

<PricingForm
  title={t("owner.devices.move", { name: line.device.name })}
  onsubmit={submit}
  {oncancel}
>
  <label class="field type-hub-label">
    <span>{t("owner.devices.move.group")}</span>
    <select class="ui-interactive control" bind:value={group}>
      <option value="">{t("owner.devices.unassigned")}</option>
      {#each options as g (g.id)}<option value={g.id}>{g.name}</option>{/each}
    </select>
  </label>
  <p class="hint type-hub-label">{t("owner.devices.move.hint")}</p>
</PricingForm>
