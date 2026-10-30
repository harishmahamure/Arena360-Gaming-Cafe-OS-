<script lang="ts">
  import { TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { Permission } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { propose } from "./commands.svelte";
  import type { BranchData, Listed } from "./pricing";
  import PricingForm from "./PricingForm.svelte";
  import { PERMISSIONS, blankRole, proposeRole, roleFormOf, validateRole } from "./staff";

  let {
    item,
    data,
    scope,
    oncancel,
  }: {
    /** The role being edited; none adds a custom role. */
    item: Listed | null;
    data: BranchData[];
    scope: string;
    oncancel: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let form = $state(item ? roleFormOf(item) : blankRole());
  let errors: Record<string, MessageKey> = $state({});
  let el: HTMLFormElement | undefined = $state();

  const title = $derived(
    item ? t("owner.roles.edit", { name: item.name }) : t("owner.roles.form.add"),
  );

  function toggle(p: Permission, on: boolean) {
    form.allowed = on ? [...form.allowed, p] : form.allowed.filter((x) => x !== p);
  }

  function submit() {
    const checked = validateRole(form);
    errors = checked.errors;
    if (!checked.ok) {
      void tick().then(() => focusFirstInvalid(el));
      return;
    }
    void propose(proposeRole({ scope, data, origin: item?.key ?? null }, checked.values));
  }
</script>

<PricingForm {title} onsubmit={submit} {oncancel} bind:form={el}>
  <TextField label="owner.roles.name" bind:value={form.name} error={errors.name} required />
  <fieldset class="field type-hub-label">
    <legend>{t("owner.roles.perms")}</legend>
    <div class="perms">
      {#each PERMISSIONS as p (p)}
        <label class="check type-hub-label"
          ><input
            type="checkbox"
            checked={form.allowed.includes(p)}
            onchange={(e) => toggle(p, e.currentTarget.checked)}
          /><code>{p}</code></label
        >
      {/each}
    </div>
  </fieldset>
</PricingForm>

<style>
  .perms {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(14rem, 1fr));
    gap: 0 var(--space-3);
  }
</style>
