<script lang="ts">
  import { TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { propose } from "./commands.svelte";
  import {
    blankCoupon,
    couponFormOf,
    exponentOf,
    groupChoices,
    norm,
    planChoices,
    productChoices,
    proposeCoupon,
    validateCoupon,
    type BranchData,
    type Choice,
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
  let form = $state(item ? couponFormOf(item) : blankCoupon());
  let errors: Record<string, MessageKey> = $state({});
  let el: HTMLFormElement | undefined = $state();

  type List = "groups" | "plans" | "products";
  const sections = $derived<{ id: List; label: MessageKey; choices: Choice[] }[]>([
    { id: "groups", label: "hub.coupons.targets.groups", choices: groupChoices(data) },
    { id: "plans", label: "hub.coupons.targets.plans", choices: planChoices(data) },
    { id: "products", label: "hub.coupons.targets.products", choices: productChoices(data) },
  ]);

  function submit() {
    const checked = validateCoupon(form, exponentOf(data));
    errors = checked.errors;
    if (!checked.ok) {
      void tick().then(() => focusFirstInvalid(el));
      return;
    }
    void propose(proposeCoupon({ scope, data, origin: item?.key ?? null }, checked.values));
  }

  function toggle(list: List, name: string) {
    form[list] = form[list].some((x) => norm(x) === norm(name))
      ? form[list].filter((x) => norm(x) !== norm(name))
      : [...form[list], name];
  }
</script>

<PricingForm
  title={item ? t("hub.coupons.edit", { code: item.name }) : t("hub.coupons.add")}
  onsubmit={submit}
  {oncancel}
  bind:form={el}
>
  <TextField label="hub.coupons.code" bind:value={form.code} error={errors.code} required />
  <fieldset class="field type-hub-label">
    <legend>{t("hub.coupons.kind")}</legend>
    <div class="inline">
      <label class="check type-hub-label"
        ><input type="radio" name="coupon-kind" value="percent" bind:group={form.kind} />{t(
          "hub.coupons.kind.percent",
        )}</label
      >
      <label class="check type-hub-label"
        ><input type="radio" name="coupon-kind" value="amount" bind:group={form.kind} />{t(
          "hub.coupons.kind.amount",
        )}</label
      >
    </div>
  </fieldset>
  <TextField
    label={form.kind === "percent" ? "hub.coupons.percent" : "hub.coupons.amount"}
    bind:value={form.value}
    error={errors.value}
    inputmode="decimal"
    required
  />
  <div class="inline">
    <label class="field type-hub-label">
      <span>{t("hub.coupons.from")}</span>
      <input
        class="ui-interactive control"
        type="date"
        bind:value={form.from}
        aria-invalid={errors.dates ? "true" : undefined}
      />
    </label>
    <label class="field type-hub-label">
      <span>{t("hub.coupons.to")}</span>
      <input
        class="ui-interactive control"
        type="date"
        bind:value={form.to}
        aria-invalid={errors.dates ? "true" : undefined}
      />
    </label>
  </div>
  {#if errors.dates}<p class="error" role="alert">{t(errors.dates)}</p>{/if}
  <p class="hint type-hub-label">{t("hub.coupons.dates_hint")}</p>
  <TextField
    label="hub.coupons.limit"
    bind:value={form.limit}
    error={errors.limit}
    inputmode="numeric"
  />
  <TextField
    label="hub.coupons.minimum"
    bind:value={form.minimum}
    error={errors.minimum}
    inputmode="decimal"
  />
  <fieldset class="field type-hub-label">
    <legend>{t("hub.coupons.targets")}</legend>
    <p class="hint type-hub-label">{t("hub.coupons.targets_hint")}</p>
    {#each sections as s (s.id)}
      {#if s.choices.length}
        <div class="field type-hub-label" role="group" aria-label={t(s.label)}>
          <span>{t(s.label)}</span>
          {#each s.choices as c (c.key)}
            <label class="check type-hub-label"
              ><input
                type="checkbox"
                checked={form[s.id].some((x) => norm(x) === c.key)}
                onchange={() => toggle(s.id, c.name)}
              />{c.name}</label
            >
          {/each}
        </div>
      {/if}
    {/each}
  </fieldset>
  <label class="check type-hub-label"
    ><input type="checkbox" bind:checked={form.active} />{t("hub.coupons.active")}</label
  >
</PricingForm>
