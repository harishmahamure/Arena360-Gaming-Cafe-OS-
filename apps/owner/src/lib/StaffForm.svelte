<script lang="ts">
  import { TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { propose } from "./commands.svelte";
  import type { BranchData } from "./pricing";
  import PricingForm from "./PricingForm.svelte";
  import {
    blankStaff,
    proposeStaffAdd,
    proposeStaffEdit,
    roleChoices,
    staffFormOf,
    validateStaff,
    type Person,
  } from "./staff";

  let {
    person,
    data,
    scope,
    oncancel,
  }: {
    /** The person being edited; none adds a new member. */
    person: Person | null;
    data: BranchData[];
    scope: string;
    oncancel: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let form = $state(person ? staffFormOf(person) : blankStaff());
  let errors: Record<string, MessageKey> = $state({});
  let el: HTMLFormElement | undefined = $state();

  const roles = $derived(roleChoices(data));
  const title = $derived(
    person ? t("owner.staff.edit", { name: person.name }) : t("owner.staff.form.add"),
  );

  function submit() {
    const checked = validateStaff(form, !person);
    errors = checked.errors;
    if (!checked.ok) {
      void tick().then(() => focusFirstInvalid(el));
      return;
    }
    const ctx = { scope, data };
    void propose(
      person ? proposeStaffEdit(ctx, person, checked.values) : proposeStaffAdd(ctx, checked.values),
    );
  }
</script>

<PricingForm {title} onsubmit={submit} {oncancel} bind:form={el}>
  <TextField label="owner.staff.name" bind:value={form.name} error={errors.name} required />
  {#if person}
    <p class="type-hub-label">{t("owner.staff.phone_is", { phone: person.login })}</p>
  {:else}
    <TextField
      label="owner.staff.phone"
      bind:value={form.phone}
      error={errors.phone}
      inputmode="tel"
      required
    />
    <p class="hint type-hub-label">{t("owner.staff.phone_hint")}</p>
  {/if}
  <label class="field type-hub-label">
    <span>{t("owner.staff.role")}</span>
    <select
      class="ui-interactive control"
      bind:value={form.role}
      aria-invalid={errors.role ? "true" : undefined}
      aria-describedby={errors.role ? "staff-role-error" : undefined}
    >
      <option value="">{t("owner.staff.role_pick")}</option>
      {#each roles as r (r.key)}<option value={r.name}>{r.name}</option>{/each}
    </select>
    {#if errors.role}<p id="staff-role-error" class="error" role="alert">
        {t(errors.role)}
      </p>{/if}
  </label>
  {#if person}
    <label class="check type-hub-label"
      ><input type="checkbox" bind:checked={form.active} />{t("owner.staff.active")}</label
    >
    <label class="check type-hub-label"
      ><input type="checkbox" bind:checked={form.newCode} />{t("owner.staff.new_code")}</label
    >
  {:else}
    <p class="hint type-hub-label">{t("owner.staff.code_hint")}</p>
  {/if}
</PricingForm>
