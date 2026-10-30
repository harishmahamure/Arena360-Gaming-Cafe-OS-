<script lang="ts">
  import { focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { propose } from "./commands.svelte";
  import { REVOKE_REASONS, proposeRevoke, type DeviceLine } from "./devices";
  import PricingForm from "./PricingForm.svelte";

  let { line, oncancel }: { line: DeviceLine; oncancel: () => void } = $props();

  let reason = $state("");
  let error: MessageKey | undefined = $state();
  let el: HTMLFormElement | undefined = $state();

  function submit() {
    error = REVOKE_REASONS.some((r) => r.id === reason)
      ? undefined
      : "owner.devices.invalid.reason";
    if (error) {
      void tick().then(() => focusFirstInvalid(el));
      return;
    }
    void propose(proposeRevoke(line, reason));
  }
</script>

<PricingForm
  title={t("owner.devices.revoke", { name: line.device.name })}
  onsubmit={submit}
  {oncancel}
  bind:form={el}
>
  <p class="type-hub-label">{t("owner.devices.revoke.warning", { name: line.device.name })}</p>
  <label class="field type-hub-label">
    <span>{t("owner.devices.reason")}</span>
    <select
      class="ui-interactive control"
      bind:value={reason}
      aria-invalid={error ? "true" : undefined}
      aria-describedby={error ? "revoke-reason-error" : undefined}
    >
      <option value="">{t("owner.devices.reason.pick")}</option>
      {#each REVOKE_REASONS as r (r.id)}<option value={r.id}>{t(r.label)}</option>{/each}
    </select>
    {#if error}<p id="revoke-reason-error" class="error" role="alert">{t(error)}</p>{/if}
  </label>
</PricingForm>
