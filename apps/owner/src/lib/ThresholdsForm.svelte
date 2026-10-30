<script lang="ts">
  import { Button, TextField, focusFirstInvalid, t } from "@arena/ui";
  import { onMount, tick } from "svelte";
  import {
    DEFAULT_LIMITS,
    formOf,
    proposeLimits,
    validateLimits,
    type LimitsErrors,
    type LimitsForm,
  } from "./alert-limits";
  import { propose } from "./commands.svelte";
  import { dataOf, exponentOf } from "./pricing";
  import { shell } from "./shell.svelte";
  import { showCached, showable } from "./today.svelte";

  // The limits are in the money unit of the branches that can take them: the strictest number of
  // decimals among them, read from the answers this device already holds.
  const paid = $derived(showable(shell.branches));
  const exponent = $derived(exponentOf(dataOf(paid)));

  // svelte-ignore state_referenced_locally
  let form: LimitsForm = $state(formOf(DEFAULT_LIMITS, exponent));
  let touched = false;
  let errors: LimitsErrors = $state({});
  let el: HTMLFormElement | undefined = $state();

  // The cached answers tell the currency; until the owner types, the fields follow its decimals.
  onMount(() => {
    void showCached(paid).then(() => {
      if (!touched) form = formOf(DEFAULT_LIMITS, exponent);
    });
  });

  function submit() {
    const checked = validateLimits(form, exponent);
    errors = checked.ok ? {} : checked.errors;
    if (!checked.ok) {
      void tick().then(() => focusFirstInvalid(el));
      return;
    }
    void propose(proposeLimits(checked.value));
  }
</script>

<section class="limits" aria-labelledby="alerts-limits-title">
  <h2 id="alerts-limits-title" class="type-hub-title">{t("owner.alerts.limits.title")}</h2>
  <p class="help type-hub-label">{t("owner.alerts.limits.help")}</p>
  <form
    bind:this={el}
    data-component="alert-limits"
    novalidate
    oninput={() => (touched = true)}
    onsubmit={(e) => {
      e.preventDefault();
      submit();
    }}
  >
    <TextField
      label="owner.alerts.limits.cash"
      bind:value={form.cash}
      error={errors.cash}
      inputmode="decimal"
    />
    <p class="hint type-hub-micro">{t("owner.alerts.limits.cash_hint")}</p>
    <TextField
      label="owner.alerts.limits.refund"
      bind:value={form.refund}
      error={errors.refund}
      inputmode="decimal"
      required
    />
    <TextField
      label="owner.alerts.limits.discount"
      bind:value={form.discount}
      error={errors.discount}
      inputmode="decimal"
      required
    />
    <TextField
      label="owner.alerts.limits.percent"
      bind:value={form.percent}
      error={errors.percent}
      inputmode="numeric"
      required
    />
    <div class="actions">
      <Button label="owner.alerts.limits.save" type="submit" />
    </div>
  </form>
</section>

<style>
  .limits,
  form {
    display: grid;
    gap: var(--space-3);
    align-content: start;
  }
  form {
    max-width: 48rem;
  }
  h2,
  p {
    margin: 0;
  }
  .help,
  .hint {
    color: var(--text-muted);
  }
</style>
