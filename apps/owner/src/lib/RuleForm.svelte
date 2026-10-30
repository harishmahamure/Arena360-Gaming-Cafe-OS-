<script lang="ts">
  import { TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { propose } from "./commands.svelte";
  import {
    DAYS,
    blankRule,
    exponentOf,
    groupChoices,
    norm,
    proposeRule,
    ruleFormOf,
    validateRule,
    type BranchData,
    type Listed,
  } from "./pricing";
  import PricingForm from "./PricingForm.svelte";

  let {
    item,
    data,
    scope,
    oncancel,
  }: { item: Listed | null; data: BranchData[]; scope: string; oncancel: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let form = $state(item ? ruleFormOf(item) : blankRule());
  let errors: Record<string, MessageKey> = $state({});
  let el: HTMLFormElement | undefined = $state();

  const groups = $derived(groupChoices(data));

  function submit() {
    const checked = validateRule(
      form,
      exponentOf(data),
      groups.map((g) => g.name),
    );
    errors = checked.errors;
    if (!checked.ok) {
      void tick().then(() => focusFirstInvalid(el));
      return;
    }
    void propose(proposeRule({ scope, data, origin: item?.key ?? null }, checked.values));
  }

  function toggleDay(bit: number) {
    form.days = form.days.includes(bit) ? form.days.filter((d) => d !== bit) : [...form.days, bit];
  }
</script>

<PricingForm
  title={item ? t("hub.rules.edit", { name: item.name }) : t("hub.rules.add")}
  onsubmit={submit}
  {oncancel}
  bind:form={el}
>
  <fieldset class="field type-hub-label">
    <legend>{t("hub.rules.type")}</legend>
    <div class="inline">
      <label class="check type-hub-label"
        ><input type="radio" name="rule-kind" value="weekly" bind:group={form.kind} />{t(
          "hub.rules.type.weekly",
        )}</label
      >
      <label class="check type-hub-label"
        ><input type="radio" name="rule-kind" value="date" bind:group={form.kind} />{t(
          "hub.rules.type.date",
        )}</label
      >
    </div>
  </fieldset>
  <TextField label="hub.rules.name" bind:value={form.name} error={errors.name} required />
  <fieldset class="field type-hub-label">
    <legend>{t("hub.rules.group")}</legend>
    {#each groups as g (g.key)}
      <label class="check type-hub-label"
        ><input
          type="radio"
          name="rule-group"
          checked={norm(form.group) === g.key}
          onchange={() => (form.group = g.name)}
        />{g.name}</label
      >
    {:else}
      <p class="hint type-hub-label">{t("hub.rules.no_groups")}</p>
    {/each}
    {#if errors.group}<p class="error" role="alert">{t(errors.group)}</p>{/if}
  </fieldset>
  {#if form.kind === "weekly"}
    <fieldset class="field type-hub-label">
      <legend>{t("hub.rules.days")}</legend>
      <div class="inline">
        {#each DAYS as d (d.bit)}
          <label class="check type-hub-label"
            ><input
              type="checkbox"
              checked={form.days.includes(d.bit)}
              onchange={() => toggleDay(d.bit)}
            />{t(d.label)}</label
          >
        {/each}
      </div>
      {#if errors.days}<p class="error" role="alert">{t(errors.days)}</p>{/if}
    </fieldset>
    <div class="inline">
      <label class="field type-hub-label">
        <span>{t("hub.rules.start")}</span>
        <input
          class="ui-interactive control"
          type="time"
          bind:value={form.start}
          aria-invalid={errors.window ? "true" : undefined}
        />
      </label>
      <label class="field type-hub-label">
        <span>{t("hub.rules.end")}</span>
        <input
          class="ui-interactive control"
          type="time"
          bind:value={form.end}
          aria-invalid={errors.window ? "true" : undefined}
        />
      </label>
    </div>
    {#if errors.window}<p class="error" role="alert">{t(errors.window)}</p>{/if}
    <p class="hint type-hub-label">{t("hub.rules.window_hint")}</p>
  {:else}
    <div class="inline">
      <label class="field type-hub-label">
        <span>{t("hub.rules.from")}</span>
        <input
          class="ui-interactive control"
          type="date"
          bind:value={form.from}
          aria-invalid={errors.dates ? "true" : undefined}
        />
      </label>
      <label class="field type-hub-label">
        <span>{t("hub.rules.to")}</span>
        <input
          class="ui-interactive control"
          type="date"
          bind:value={form.to}
          aria-invalid={errors.dates ? "true" : undefined}
        />
      </label>
    </div>
    {#if errors.dates}<p class="error" role="alert">{t(errors.dates)}</p>{/if}
    <p class="hint type-hub-label">{t("hub.rules.date_hint")}</p>
  {/if}
  <fieldset class="field type-hub-label">
    <legend>{t("hub.rules.adjust")}</legend>
    <div class="inline">
      <label class="check type-hub-label"
        ><input type="radio" name="rule-adjust" value="percent_bp" bind:group={form.adjust} />{t(
          "hub.rules.adjust.percent",
        )}</label
      >
      <label class="check type-hub-label"
        ><input
          type="radio"
          name="rule-adjust"
          value="override_minor"
          bind:group={form.adjust}
        />{t("hub.rules.adjust.override")}</label
      >
    </div>
  </fieldset>
  <TextField
    label={form.adjust === "percent_bp" ? "hub.rules.percent" : "hub.rules.rate"}
    bind:value={form.value}
    error={errors.adjust}
    inputmode="decimal"
    required
  />
  <TextField
    label="hub.rules.priority"
    bind:value={form.priority}
    error={errors.priority}
    inputmode="numeric"
  />
  <label class="check type-hub-label"
    ><input type="checkbox" bind:checked={form.active} />{t("hub.rules.active")}</label
  >
</PricingForm>
