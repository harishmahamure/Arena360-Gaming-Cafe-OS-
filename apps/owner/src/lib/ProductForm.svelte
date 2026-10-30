<script lang="ts">
  import { TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import { propose } from "./commands.svelte";
  import { shrinkImage } from "./image";
  import { exponentOf, type BranchData, type Listed } from "./pricing";
  import PricingForm from "./PricingForm.svelte";
  import {
    blankProduct,
    partChoices,
    productFormOf,
    proposeProduct,
    validateProduct,
  } from "./products";

  let {
    item,
    bundle,
    data,
    scope,
    oncancel,
  }: {
    item: Listed | null;
    /** A new item is a bundle; an edited one keeps what it is. */
    bundle: boolean;
    data: BranchData[];
    scope: string;
    oncancel: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let form = $state(item ? productFormOf(item) : blankProduct(bundle));
  let errors: Record<string, MessageKey> = $state({});
  let el: HTMLFormElement | undefined = $state();
  // The picture the owner chose, to show it; the form keeps the data URL.
  let imageError = $state(false);

  const parts = $derived(partChoices(data));
  const title = $derived(
    item
      ? t("hub.products.edit", { name: item.name })
      : t(form.bundle ? "owner.products.add_bundle" : "hub.products.add"),
  );

  function submit() {
    const checked = validateProduct(form, exponentOf(data));
    errors = checked.errors;
    if (imageError) errors.image = "hub.products.invalid.image";
    if (!checked.ok || imageError) {
      void tick().then(() => focusFirstInvalid(el));
      return;
    }
    void propose(proposeProduct({ scope, data, origin: item?.key ?? null }, checked.values));
  }

  async function pick(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    const image = await shrinkImage(file);
    imageError = image === null;
    if (image !== null) form.image = image;
    else input.value = "";
  }

  function removeImage() {
    form.image = "";
    imageError = false;
  }

  const addPart = () => (form.parts = [...form.parts, { name: "", qty: "1" }]);
  const dropPart = (i: number) => (form.parts = form.parts.filter((_, n) => n !== i));
</script>

<PricingForm {title} onsubmit={submit} {oncancel} bind:form={el}>
  <TextField label="hub.products.name" bind:value={form.name} error={errors.name} required />
  <TextField label="hub.products.category" bind:value={form.category} error={errors.category} />
  <TextField
    label="hub.products.price"
    bind:value={form.price}
    error={errors.price}
    inputmode="decimal"
    required
  />
  <TextField
    label="hub.products.tax"
    bind:value={form.tax}
    error={errors.tax}
    inputmode="decimal"
  />
  <p class="hint type-hub-label">{t("owner.products.tax_hint")}</p>
  <TextField
    label="hub.products.hsn"
    bind:value={form.hsn}
    error={errors.hsn}
    inputmode="numeric"
  />
  <TextField label="hub.products.barcode" bind:value={form.barcode} error={errors.barcode} />
  {#if form.bundle}
    <fieldset class="field type-hub-label">
      <legend>{t("hub.products.components")}</legend>
      {#each form.parts as part, i (i)}
        <div class="inline">
          <label class="field type-hub-label">
            <span>{t("hub.products.component", { n: i + 1 })}</span>
            <select
              class="ui-interactive control"
              bind:value={part.name}
              aria-invalid={errors.parts ? "true" : undefined}
            >
              <option value="">{t("hub.products.component_pick")}</option>
              {#each parts as c (c.key)}<option value={c.name}>{c.name}</option>{/each}
            </select>
          </label>
          <label class="field type-hub-label">
            <span>{t("hub.products.component_qty", { n: i + 1 })}</span>
            <input
              class="ui-interactive control qty"
              inputmode="numeric"
              bind:value={part.qty}
              aria-invalid={errors.parts ? "true" : undefined}
            />
          </label>
          <button
            type="button"
            class="ui-interactive control type-hub-label"
            onclick={() => dropPart(i)}>{t("hub.products.component_remove", { n: i + 1 })}</button
          >
        </div>
      {/each}
      <div>
        <button type="button" class="ui-interactive control type-hub-label" onclick={addPart}
          >{t("hub.products.component_add")}</button
        >
      </div>
      {#if errors.parts}<p class="error" role="alert">{t(errors.parts)}</p>{/if}
    </fieldset>
  {:else}
    <label class="check type-hub-label"
      ><input type="checkbox" bind:checked={form.track} />{t("hub.products.track")}</label
    >
  {/if}
  <label class="check type-hub-label"
    ><input type="checkbox" bind:checked={form.kitchen} />{t("hub.products.kitchen")}</label
  >
  <label class="check type-hub-label"
    ><input type="checkbox" bind:checked={form.favourite} />{t("hub.products.favourite")}</label
  >
  <label class="check type-hub-label"
    ><input type="checkbox" bind:checked={form.active} />{t("hub.products.active")}</label
  >
  <div class="field image type-hub-label">
    <label for="product-image">{t("hub.products.image")}</label>
    <input
      id="product-image"
      class="ui-interactive control"
      type="file"
      accept="image/*"
      onchange={pick}
      aria-invalid={imageError ? "true" : undefined}
    />
    {#if form.image}
      <img class="picture" src={form.image} alt="" />
    {:else if form.image === undefined && item}
      <p class="hint type-hub-label">{t("owner.products.image_kept")}</p>
    {/if}
    {#if form.image !== ""}
      <div>
        <button type="button" class="ui-interactive control type-hub-label" onclick={removeImage}
          >{t("hub.products.image_remove")}</button
        >
      </div>
    {:else}
      <p class="hint type-hub-label">{t("owner.products.image_removed")}</p>
    {/if}
    {#if imageError}<p class="error" role="alert">{t("hub.products.invalid.image")}</p>{/if}
  </div>
</PricingForm>

<style>
  .image {
    grid-template-columns: minmax(0, 1fr);
  }
  .image input {
    max-inline-size: 100%;
  }
  .picture {
    max-inline-size: 8rem;
    max-block-size: 8rem;
    border-radius: var(--radius-sm);
  }
  .qty {
    inline-size: 6rem;
  }
</style>
