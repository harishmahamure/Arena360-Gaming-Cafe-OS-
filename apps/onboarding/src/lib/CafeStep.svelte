<script lang="ts">
  import { Button, CURRENCIES, TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { Country } from "@arena/ui/generated";
  import { goto } from "$app/navigation";
  import { onMount, tick } from "svelte";
  import {
    currentSession,
    getWizard,
    listCountries,
    listTimezones,
    saveCafe,
    setBranchCode,
    type ApiFailure,
  } from "./api";
  import { normalizeGstin, validGstin } from "./gstin";
  import { validText } from "./validate";
  import { STEP_NAMES, advance, setWizard, wizard } from "./wizard.svelte";

  type Field = "cafe_name" | "city" | "country" | "currency" | "timezone" | "gst" | "gstin";
  const FIELDS: readonly Field[] = [
    "cafe_name",
    "city",
    "country",
    "currency",
    "timezone",
    "gst",
    "gstin",
  ];
  const SERVER_FIELDS: Record<string, Field> = {
    cafe_name: "cafe_name",
    city: "city",
    country: "country",
    currency: "currency",
    timezone: "timezone",
    gst_registered: "gst",
    gstin: "gstin",
  };

  const id = $props.id();
  const errId = (field: string) => id + "-" + field + "-error";
  const GST_LABELS: Record<"yes" | "no", MessageKey> = {
    yes: "onboarding.cafe.gst.yes",
    no: "onboarding.cafe.gst.no",
  };

  let countries: Country[] = $state([]);
  let timezones: string[] = $state([]);
  let ready = $state(false);
  let loadError: MessageKey | null = $state(null);

  let form = $state({
    cafe_name: "",
    city: "",
    country: "IN",
    currency: "INR",
    timezone: "Asia/Kolkata",
    gst: "no" as "yes" | "no",
    gstin: "",
  });
  let errors: Partial<Record<Field, MessageKey>> = $state({});
  let formError: MessageKey | null = $state(null);
  let saving = $state(false);
  let formEl: HTMLFormElement | undefined = $state();

  let editingCode = $state(false);
  let codeValue = $state("");
  let codeError: MessageKey | null = $state(null);
  let codeStatus: MessageKey | null = $state(null);
  let codeSaving = $state(false);
  let codeField: HTMLDivElement | undefined = $state();
  let editButton: HTMLDivElement | undefined = $state();

  const branch = $derived(wizard.state?.branch ?? null);
  const selected = $derived(countries.find((c) => c.code === form.country) ?? null);
  const gstApplies = $derived(selected?.gst ?? false);
  const zoneOptions = $derived(
    timezones.includes(form.timezone) || !form.timezone ? timezones : [form.timezone, ...timezones],
  );

  function failure(f: ApiFailure): MessageKey {
    return f.offline ? "onboarding.error.offline" : "onboarding.error.generic";
  }

  async function load() {
    loadError = null;
    const [c, z] = await Promise.all([listCountries(), listTimezones()]);
    if (!c.ok || !z.ok) {
      loadError = failure((!c.ok ? c : z) as ApiFailure);
      return;
    }
    countries = c.data;
    timezones = z.data;
    const b = wizard.state?.branch;
    if (b) {
      form = {
        cafe_name: wizard.state?.tenant?.name ?? b.name,
        city: b.city,
        country: b.country,
        currency: b.currency,
        timezone: b.timezone,
        gst: b.gst_registered ? "yes" : "no",
        gstin: b.gstin ?? "",
      };
    } else {
      const s = await currentSession();
      if (!s.ok) {
        if (s.status === 401) {
          await goto("/", { replaceState: true });
          return;
        }
        loadError = failure(s);
        return;
      }
      form.cafe_name = s.data.owner.cafe_name;
      form.city = s.data.owner.city;
      const first = countries[0];
      if (first) chooseCountry(first.code);
    }
    ready = true;
  }

  onMount(load);

  function chooseCountry(code: string) {
    form.country = code;
    const c = countries.find((x) => x.code === code);
    if (!c) return;
    form.currency = c.currency;
    form.timezone = c.timezone;
    if (!c.gst) {
      form.gst = "no";
      form.gstin = "";
      delete errors.gst;
      delete errors.gstin;
    }
    delete errors.country;
    delete errors.currency;
    delete errors.timezone;
  }

  function fieldError(field: Field): MessageKey | null {
    switch (field) {
      case "cafe_name":
        return validText(form.cafe_name) ? null : "onboarding.error.cafe_name";
      case "city":
        return validText(form.city) ? null : "onboarding.error.city";
      case "country":
        return countries.some((c) => c.code === form.country)
          ? null
          : "onboarding.cafe.error.country";
      case "currency":
        return CURRENCIES.some((c) => c.code === form.currency)
          ? null
          : "onboarding.cafe.error.currency";
      case "timezone":
        return form.timezone ? null : "onboarding.cafe.error.timezone";
      case "gst":
        return form.gst === "yes" && !gstApplies ? "onboarding.cafe.error.gst" : null;
      case "gstin": {
        if (form.gst !== "yes") return null;
        const g = normalizeGstin(form.gstin);
        if (!g) return "onboarding.cafe.error.gstin_required";
        return validGstin(g) ? null : "onboarding.cafe.error.gstin_invalid";
      }
    }
  }

  function validate(field: Field) {
    const e = fieldError(field);
    if (e) errors[field] = e;
    else delete errors[field];
  }

  function blurred(field: "cafe_name" | "city" | "gstin", event: FocusEvent) {
    const to = event.relatedTarget;
    if (to instanceof HTMLButtonElement && to.type === "submit") return;
    if (form[field].trim() !== "" || errors[field]) validate(field);
  }

  function setGst(value: "yes" | "no") {
    form.gst = value;
    if (value === "no") {
      form.gstin = "";
      delete errors.gstin;
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    formError = null;
    for (const f of FIELDS) validate(f);
    if (Object.keys(errors).length > 0) {
      await tick();
      focusFirstInvalid(formEl);
      return;
    }
    saving = true;
    const gst = form.gst === "yes";
    const res = await saveCafe({
      cafe_name: form.cafe_name.trim(),
      city: form.city.trim(),
      country: form.country,
      currency: form.currency,
      timezone: form.timezone,
      gst_registered: gst,
      ...(gst ? { gstin: normalizeGstin(form.gstin) } : {}),
    });
    saving = false;
    if (res.ok) {
      await advance(res.data);
      return;
    }
    if (res.error.code === "SETUP_INCOMPLETE") {
      await goto("/account-setup", { replaceState: true });
      return;
    }
    if (res.error.code === "AUTH_FAILED") {
      await goto("/", { replaceState: true });
      return;
    }
    const field = res.error.field ? SERVER_FIELDS[res.error.field] : undefined;
    if (res.error.code === "VALIDATION_FAILED" && field) {
      errors[field] = fieldError(field) ?? serverError(field);
      await tick();
      focusFirstInvalid(formEl);
      return;
    }
    formError = failure(res);
  }

  function serverError(field: Field): MessageKey {
    const keys: Record<Field, MessageKey> = {
      cafe_name: "onboarding.error.cafe_name",
      city: "onboarding.error.city",
      country: "onboarding.cafe.error.country",
      currency: "onboarding.cafe.error.currency",
      timezone: "onboarding.cafe.error.timezone",
      gst: "onboarding.cafe.error.gst",
      gstin: "onboarding.cafe.error.gstin_invalid",
    };
    return keys[field];
  }

  async function startCodeEdit() {
    if (!branch) return;
    codeValue = branch.branch_code;
    codeError = null;
    codeStatus = null;
    editingCode = true;
    await tick();
    codeField?.querySelector("input")?.focus();
  }

  async function cancelCodeEdit() {
    editingCode = false;
    codeError = null;
    await tick();
    editButton?.querySelector("button")?.focus();
  }

  async function saveCode(event: SubmitEvent) {
    event.preventDefault();
    if (!branch || codeSaving) return;
    const code = codeValue.trim().toUpperCase();
    codeStatus = null;
    if (!/^[A-Z0-9]{3,6}$/.test(code)) {
      codeError = "onboarding.cafe.code.invalid";
      codeField?.querySelector("input")?.focus();
      return;
    }
    codeSaving = true;
    const res = await setBranchCode(branch.id, code);
    codeSaving = false;
    if (res.ok) {
      if (wizard.state?.branch) wizard.state.branch.branch_code = res.data.branch_code;
      editingCode = false;
      codeError = null;
      codeStatus = "onboarding.cafe.code.saved";
      await tick();
      editButton?.querySelector("button")?.focus();
      return;
    }
    if (res.error.code === "ALREADY_SET") {
      editingCode = false;
      codeStatus = "onboarding.cafe.code.locked";
      const fresh = await getWizard();
      if (fresh.ok) setWizard(fresh.data);
      else if (wizard.state?.branch) wizard.state.branch.code_editable = false;
      return;
    }
    codeError =
      res.error.code === "BRANCH_CODE_TAKEN"
        ? "onboarding.cafe.code.taken"
        : res.error.code === "VALIDATION_FAILED"
          ? "onboarding.cafe.code.invalid"
          : failure(res);
    codeField?.querySelector("input")?.focus();
  }
</script>

<h1 tabindex="-1">{t(STEP_NAMES.cafe)}</h1>
<p class="intro">{t("onboarding.cafe.intro")}</p>

{#if loadError}
  <p class="form-error" role="alert">
    <span class="glyph" aria-hidden="true">⚠</span><span>{t(loadError)}</span>
  </p>
  <div class="actions">
    <Button label="onboarding.account_setup.retry" onclick={load} />
  </div>
{:else if !ready}
  <p role="status">{t("onboarding.setup.loading")}</p>
{:else}
  {#if branch}
    <section class="code-card" aria-labelledby="{id}-code-title">
      <h2 id="{id}-code-title" class="code-title">{t("onboarding.cafe.code.label")}</h2>
      {#if editingCode}
        <form class="code-form" novalidate onsubmit={saveCode}>
          <div bind:this={codeField}>
            <TextField
              label="onboarding.cafe.code.field"
              name="branch_code"
              autocomplete="off"
              bind:value={codeValue}
              error={codeError}
            />
          </div>
          <div class="actions">
            <Button
              label="onboarding.cafe.code.save"
              type="submit"
              disabled={codeSaving}
              disabledReason="onboarding.cafe.saving"
            />
            <Button
              label="onboarding.cafe.code.cancel"
              variant="secondary"
              onclick={cancelCodeEdit}
            />
          </div>
        </form>
      {:else}
        <p class="code" data-testid="branch-code">{branch.branch_code}</p>
        <p class="hint">
          {t(
            branch.code_editable ? "onboarding.cafe.code.hint" : "onboarding.cafe.code.locked_hint",
          )}
        </p>
        {#if branch.code_editable}
          <div class="actions" bind:this={editButton}>
            <Button label="onboarding.cafe.code.edit" variant="secondary" onclick={startCodeEdit} />
          </div>
        {/if}
      {/if}
      <p class="status" role="status">
        {#if codeStatus}{t(codeStatus)}{/if}
      </p>
    </section>
  {/if}

  <form class="form" bind:this={formEl} novalidate onsubmit={submit}>
    <div class="field" onfocusout={(e) => blurred("cafe_name", e)}>
      <TextField
        label="onboarding.field.cafe_name"
        name="cafe_name"
        autocomplete="organization"
        required
        bind:value={form.cafe_name}
        error={errors.cafe_name ?? null}
      />
    </div>
    <div class="field" onfocusout={(e) => blurred("city", e)}>
      <TextField
        label="onboarding.field.city"
        name="city"
        autocomplete="address-level2"
        required
        bind:value={form.city}
        error={errors.city ?? null}
      />
    </div>
    <div class="select-field">
      <label class="label" for="{id}-country">{t("onboarding.cafe.field.country")}</label>
      <select
        id="{id}-country"
        class="ui-interactive select"
        name="country"
        autocomplete="country"
        value={form.country}
        aria-invalid={errors.country ? "true" : undefined}
        aria-describedby={errors.country ? errId("country") : undefined}
        onchange={(e) => chooseCountry(e.currentTarget.value)}
      >
        {#each countries as c (c.code)}
          <option value={c.code}>{c.name}</option>
        {/each}
      </select>
      {#if errors.country}<p class="error" id={errId("country")}>{t(errors.country)}</p>{/if}
    </div>
    <p class="hint">{t("onboarding.cafe.defaults_hint")}</p>
    <div class="pair">
      <div class="select-field">
        <label class="label" for="{id}-currency">{t("onboarding.cafe.field.currency")}</label>
        <select
          id="{id}-currency"
          class="ui-interactive select"
          name="currency"
          bind:value={form.currency}
          aria-invalid={errors.currency ? "true" : undefined}
          aria-describedby={errors.currency ? errId("currency") : undefined}
          onchange={() => validate("currency")}
        >
          {#each CURRENCIES as c (c.code)}
            <option value={c.code}>{c.code} ({c.symbol})</option>
          {/each}
        </select>
        {#if errors.currency}<p class="error" id={errId("currency")}>{t(errors.currency)}</p>{/if}
      </div>
      <div class="select-field">
        <label class="label" for="{id}-timezone">{t("onboarding.cafe.field.timezone")}</label>
        <select
          id="{id}-timezone"
          class="ui-interactive select"
          name="timezone"
          bind:value={form.timezone}
          aria-invalid={errors.timezone ? "true" : undefined}
          aria-describedby={errors.timezone ? errId("timezone") : undefined}
          onchange={() => validate("timezone")}
        >
          {#each zoneOptions as z (z)}
            <option value={z}>{z}</option>
          {/each}
        </select>
        {#if errors.timezone}<p class="error" id={errId("timezone")}>{t(errors.timezone)}</p>{/if}
      </div>
    </div>
    {#if gstApplies}
      <fieldset class="gst" aria-describedby={errors.gst ? errId("gst") : undefined}>
        <legend class="label">{t("onboarding.cafe.gst.legend")}</legend>
        <div class="choices">
          {#each ["yes", "no"] as const as value (value)}
            <div class="choice">
              <span class="radio">
                <input
                  id="{id}-gst-{value}"
                  type="radio"
                  name="gst_registered"
                  {value}
                  checked={form.gst === value}
                  onchange={() => setGst(value)}
                />
                <span class="dot" aria-hidden="true"></span>
              </span>
              <label for="{id}-gst-{value}">{t(GST_LABELS[value])}</label>
            </div>
          {/each}
        </div>
        {#if errors.gst}<p class="error" id={errId("gst")}>{t(errors.gst)}</p>{/if}
      </fieldset>
      {#if form.gst === "yes"}
        <div class="field" onfocusout={(e) => blurred("gstin", e)}>
          <TextField
            label="onboarding.cafe.field.gstin"
            name="gstin"
            autocomplete="off"
            required
            bind:value={form.gstin}
            error={errors.gstin ?? null}
          />
          <p class="hint">{t("onboarding.cafe.gstin_hint")}</p>
        </div>
      {/if}
    {/if}
    {#if formError}
      <p class="form-error" role="alert">
        <span class="glyph" aria-hidden="true">⚠</span><span>{t(formError)}</span>
      </p>
    {/if}
    <div class="actions">
      <Button
        label="onboarding.cafe.save"
        type="submit"
        disabled={saving}
        disabledReason="onboarding.cafe.saving"
      />
    </div>
  </form>
{/if}

<style>
  .intro {
    margin: 0 0 var(--space-5);
    line-height: var(--type-hub-label-line-height);
  }

  .form,
  .code-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .code-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-bottom: var(--space-5);
    padding: var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
  }

  .code-title {
    margin: 0;
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .code {
    margin: 0;
    font-family: var(--type-web-display-family), system-ui, sans-serif;
    font-size: var(--type-web-display-size);
    font-weight: var(--type-web-display-weight);
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.08em;
  }

  .pair {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .pair .select-field {
    flex: 1 1 12rem;
  }

  .select-field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }

  .label {
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
    line-height: var(--type-hub-label-line-height);
  }

  .select {
    --ui-bg: var(--text-field-background);
    min-height: var(--space-touch-min);
    width: 100%;
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
    font-size: var(--type-web-body-size);
  }

  .select[aria-invalid="true"] {
    border-width: 2px;
    border-color: var(--danger);
  }

  .hint {
    margin: 0;
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
  }

  .gst {
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }

  .gst legend {
    padding: 0;
    margin-bottom: var(--space-2);
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
  }

  .choice {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .choice label {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    cursor: pointer;
  }

  .radio {
    position: relative;
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: var(--space-touch-min);
    height: var(--space-touch-min);
  }

  .radio input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }

  .dot {
    box-sizing: border-box;
    width: 1.5rem;
    height: 1.5rem;
    border: 2px solid var(--border-strong);
    border-radius: 50%;
    background: var(--text-field-background);
    pointer-events: none;
  }

  .radio:has(input:checked) .dot {
    border: 0.45rem solid var(--accent);
  }

  .radio:has(input:focus-visible) .dot {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .error,
  .form-error {
    display: flex;
    gap: var(--space-2);
    margin: 0;
    color: var(--text-field-error-text);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .form-error {
    min-height: 1lh;
  }

  .form-error:empty {
    min-height: 0;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .status {
    margin: 0;
  }

  .status:empty {
    display: none;
  }
</style>
