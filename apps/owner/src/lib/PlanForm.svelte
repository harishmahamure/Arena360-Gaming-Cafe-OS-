<script lang="ts">
  import { TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { propose } from "./commands.svelte";
  import {
    blankPlan,
    exponentOf,
    groupChoices,
    norm,
    planFormOf,
    proposePlan,
    validatePlan,
    type BranchData,
    type Listed,
    type PlanKind,
  } from "./pricing";
  import PricingForm from "./PricingForm.svelte";

  let {
    item,
    data,
    scope,
    oncancel,
  }: { item: Listed | null; data: BranchData[]; scope: string; oncancel: () => void } = $props();

  const KINDS: { id: PlanKind; label: MessageKey }[] = [
    { id: "pack", label: "hub.plans.kind.pack" },
    { id: "block", label: "hub.plans.kind.block" },
    { id: "pass", label: "hub.plans.kind.pass" },
  ];

  // svelte-ignore state_referenced_locally
  let form = $state(item ? planFormOf(item) : blankPlan());
  let errors: Record<string, MessageKey> = $state({});
  let el: HTMLFormElement | undefined = $state();

  const groups = $derived(groupChoices(data));
  const kinds = $derived(new Map(groups.map((g) => [g.key, g.kind ?? ""])));

  function submit() {
    const checked = validatePlan(form, exponentOf(data), kinds);
    errors = checked.errors;
    if (!checked.ok) {
      void tick().then(() => focusFirstInvalid(el));
      return;
    }
    void propose(proposePlan({ scope, data, origin: item?.key ?? null }, checked.values));
  }

  function toggle(name: string) {
    form.groups = form.groups.some((g) => norm(g) === norm(name))
      ? form.groups.filter((g) => norm(g) !== norm(name))
      : [...form.groups, name];
  }
</script>

<PricingForm
  title={item ? t("hub.plans.edit", { name: item.name }) : t("hub.plans.add")}
  onsubmit={submit}
  {oncancel}
  bind:form={el}
>
  <fieldset class="field type-hub-label" disabled={item !== null}>
    <legend>{t("hub.plans.kind")}</legend>
    <div class="inline">
      {#each KINDS as k (k.id)}
        <label class="check type-hub-label"
          ><input type="radio" name="plan-kind" value={k.id} bind:group={form.kind} />{t(
            k.label,
          )}</label
        >
      {/each}
    </div>
  </fieldset>
  <TextField label="hub.plans.name" bind:value={form.name} error={errors.name} required />
  <TextField
    label="hub.plans.price"
    bind:value={form.price}
    error={errors.price}
    inputmode="decimal"
    required
  />
  {#if form.kind === "pack"}
    <TextField
      label="hub.plans.hours"
      bind:value={form.hours}
      error={errors.hours}
      inputmode="decimal"
      required
    />
  {/if}
  {#if form.kind !== "block"}
    <TextField
      label="hub.plans.validity"
      bind:value={form.days}
      error={errors.days}
      inputmode="numeric"
      required
    />
  {/if}
  {#if form.kind !== "pack"}
    <div class="inline">
      <label class="field type-hub-label">
        <span>{t(form.kind === "block" ? "hub.plans.window_start" : "hub.plans.play_start")}</span>
        <input
          class="ui-interactive control"
          type="time"
          bind:value={form.start}
          aria-invalid={errors.window ? "true" : undefined}
          aria-describedby={errors.window ? "plan-window-error" : undefined}
        />
      </label>
      <label class="field type-hub-label">
        <span>{t(form.kind === "block" ? "hub.plans.window_end" : "hub.plans.play_end")}</span>
        <input
          class="ui-interactive control"
          type="time"
          bind:value={form.end}
          aria-invalid={errors.window ? "true" : undefined}
          aria-describedby={errors.window ? "plan-window-error" : undefined}
        />
      </label>
    </div>
    {#if errors.window}<p class="error" id="plan-window-error" role="alert">
        {t(errors.window)}
      </p>{/if}
    <p class="hint type-hub-label">
      {t(form.kind === "block" ? "hub.plans.block_hint" : "hub.plans.pass_hint")}
    </p>
  {/if}
  <fieldset class="field type-hub-label">
    <legend>{t("hub.plans.groups")}</legend>
    {#each groups as g (g.key)}
      <label class="check type-hub-label"
        ><input
          type="checkbox"
          checked={form.groups.some((x) => norm(x) === g.key)}
          onchange={() => toggle(g.name)}
        />{g.name}</label
      >
    {:else}
      <p class="hint type-hub-label">{t("hub.plans.no_groups")}</p>
    {/each}
    {#if errors.groups}<p class="error" role="alert">{t(errors.groups)}</p>{/if}
  </fieldset>
  <label class="check type-hub-label"
    ><input type="checkbox" bind:checked={form.active} />{t("hub.plans.active")}</label
  >
</PricingForm>
