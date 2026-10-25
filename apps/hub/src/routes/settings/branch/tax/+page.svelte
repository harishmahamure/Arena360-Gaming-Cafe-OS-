<script lang="ts">
  import { Button, InlinePin, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    dropElevation,
    elevate,
    hasPermission,
    loadSettings,
    newIdempotencyKey,
    submitIntent,
  } from "../../../../lib/auth";
  import { branchLocale } from "../../../../lib/locale.svelte";
  import { sessionStaff, snapshotOf } from "../../../../lib/operating";
  import { SNAPSHOT_DEPENDENCY } from "../../../../lib/snapshot";
  import {
    KEY_TAX,
    fieldName,
    parsePct,
    parseTax,
    pct,
    taxJson,
    type Tax,
  } from "../../../../lib/tax";

  const BLANK = {
    country: "",
    gst: false,
    gstin: "",
    inclusive: "true",
    product: "",
    play: "",
    sac: "",
    label: "",
    rate: "",
  };

  const INVALID: Record<string, MessageKey> = {
    country: "hub.settings.tax.invalid.country",
    gst: "hub.settings.tax.invalid.gst",
    gstin: "hub.settings.tax.invalid.gstin",
    product: "hub.settings.tax.invalid.rate",
    play: "hub.settings.tax.invalid.rate",
    rate: "hub.settings.tax.invalid.rate",
    sac: "hub.settings.tax.invalid.sac",
    label: "hub.settings.tax.invalid.label",
  };
  const LABELS = {
    country: "hub.settings.tax.country",
    gstin: "hub.settings.tax.gstin",
    product: "hub.settings.tax.product",
    play: "hub.settings.tax.play",
    sac: "hub.settings.tax.sac",
    label: "hub.settings.tax.label",
    rate: "hub.settings.tax.rate",
  } as const satisfies Record<string, MessageKey>;

  let server = $state<{ value: string; rev: number } | null>(null);
  let form = $state({ ...BLANK });
  let errors: Record<string, MessageKey> = $state({});
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let formEl: HTMLFormElement | undefined = $state();
  let saveWrap: HTMLElement | undefined = $state();
  let idem: { value: string; key: string } | null = null;

  const staff = $derived(sessionStaff(page.data));
  const canEdit = $derived(staff !== null);
  const needsApproval = $derived(!hasPermission(staff, "settings.edit"));
  const locked = $derived(saving || approving);
  const saveButton = $derived(saveWrap?.querySelector("button") ?? null);
  const code = $derived(snapshotOf(page.data)?.branch?.branch_code ?? "");
  const codeLocked = $derived(snapshotOf(page.data)?.branch?.code_locked === true);
  const india = $derived(form.country.trim().toUpperCase() === "IN");
  const base = $derived(server ? parseTax(server.value) : null);
  const draft = $derived(base && taxJson(base, form));
  const changed = $derived(!!server && !!draft && draft !== taxJson(base!, fromTax(base!)));

  function fromTax(x: Tax) {
    return {
      country: x.country,
      gst: x.gst,
      gstin: x.gstin,
      inclusive: String(x.inclusive),
      product: pct(x.product_bp),
      play: pct(x.play_bp),
      sac: x.sac,
      label: x.label,
      rate: pct(x.rate_bp),
    };
  }

  async function load() {
    loading = true;
    try {
      const result = await loadSettings();
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const row = result.kind === "ok" ? result.settings.find((s) => s.key === KEY_TAX) : null;
      const tax = row && parseTax(row.value);
      if (!row || !tax) return void (loadFailed = true);
      loadFailed = false;
      server = { value: row.value, rev: row.rev };
      form = fromTax(tax);
    } finally {
      loading = false;
    }
  }

  async function approve(pin: string) {
    const outcome = await elevate([{ intent_kind: "settings.set", key: KEY_TAX }], pin);
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind === "unreachable")
      return { ok: false, message: "ui.pin.unreachable" } as const;
    if (outcome.kind === "error") return { ok: false, message: "ui.pin.failed" } as const;
    if (!approving) {
      void dropElevation(outcome.ticket);
      return { ok: true } as const;
    }
    approving = false;
    void save(outcome.ticket);
    return { ok: true } as const;
  }

  async function save(ticket?: string) {
    if (!canEdit || saving || loading || !changed || !server) {
      if (ticket) void dropElevation(ticket);
      return;
    }
    const bad = (["product", "play", "rate"] as const).find(
      (f) => (f === "rate" ? !india : india) && parsePct(form[f]) === null,
    );
    if (bad) {
      errors = { [bad]: "hub.settings.tax.invalid.rate" };
      status = null;
      await tick();
      return focusFirstInvalid(formEl);
    }
    if (needsApproval && !ticket) {
      approvalPrompt = "ui.pin.prompt";
      approving = true;
      return;
    }
    saving = true;
    status = null;
    errors = {};
    let again = false;
    let done = false;
    try {
      const value = draft!;
      if (idem?.value !== value) idem = { value, key: newIdempotencyKey() };
      const outcome = await submitIntent(
        { kind: "settings.set", key: KEY_TAX, value },
        idem.key,
        server.rev,
        { elevationTicket: ticket },
      );
      if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (outcome.kind === "applied") {
        idem = null;
        done = true;
        await load();
        status = { tone: "ok", message: "hub.settings.saved" };
        return;
      }
      const err = outcome.kind === "error" ? outcome.error : null;
      if (err?.code === "VALIDATION_FAILED") {
        idem = null;
        const name = fieldName(err.field);
        if (INVALID[name]) errors = { [name]: INVALID[name] };
        else status = { tone: "refusal", message: "hub.settings.failed" };
      } else if (err?.code === "STALE") {
        idem = null;
        status = { tone: "refusal", message: "hub.settings.branch.stale" };
        void invalidate(SNAPSHOT_DEPENDENCY);
        await load();
      } else if (err?.code === "PIN_REQUIRED") {
        again = true;
      } else if (err?.code === "PERMISSION_DENIED") {
        idem = null;
        status = { tone: "refusal", message: "hub.settings.branch.manager_only" };
      } else {
        status = { tone: "refusal", message: "hub.settings.failed" };
      }
    } finally {
      saving = false;
      if (ticket && !again && !done) void dropElevation(ticket);
    }
    if (again) {
      approvalPrompt = "ui.pin.again";
      approving = true;
    } else if (Object.keys(errors).length) {
      await tick();
      focusFirstInvalid(formEl);
    }
  }

  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      if (!saving && !changed && branchLocale.changedKeys.includes(KEY_TAX)) void load();
    });
  });

  onMount(() => {
    void load();
  });

  const describedBy = (f: string) => (errors[f] ? "tax-" + f + "-error" : undefined);
</script>

{#snippet fieldError(f: string)}
  <p class="error type-hub-label" id={"tax-" + f + "-error"} aria-live="polite">
    {#if errors[f]}<span aria-hidden="true">⚠</span>
      <span>{t(errors[f]!)}</span>{/if}
  </p>
{/snippet}

{#snippet text(
  f: "country" | "gstin" | "product" | "play" | "sac" | "label" | "rate",
  mode: "text" | "decimal" = "text",
)}
  <div class="field">
    <label class="label type-hub-label" for={"tax-" + f}>{t(LABELS[f])}</label>
    <input
      id={"tax-" + f}
      class="control ui-interactive"
      class:invalid={!!errors[f]}
      inputmode={mode}
      autocomplete="off"
      bind:value={form[f]}
      disabled={!canEdit || locked}
      aria-invalid={errors[f] ? "true" : undefined}
      aria-describedby={describedBy(f)}
    />
    {@render fieldError(f)}
  </div>
{/snippet}

<section class="tax" aria-labelledby="settings-tax-title">
  <h2 class="type-hub-title" id="settings-tax-title">{t("hub.settings.tax.title")}</h2>
  <p class="intro type-hub-body">{t("hub.settings.tax.intro")}</p>

  {#if loadFailed}
    <div class="actions" data-testid="tax-load-failed">
      <p class="status refusal type-hub-label" role="status">
        <span aria-hidden="true">⚠</span>
        <span>{t("hub.settings.load_failed")}</span>
      </p>
      <Button
        label="common.retry"
        variant="secondary"
        disabled={loading}
        disabledReason="hub.settings.loading"
        onclick={() => void load()}
      />
    </div>
  {:else}
    <form
      class="form"
      data-testid="settings-tax"
      novalidate
      bind:this={formEl}
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      {@render text("country")}
      {#if india}
        <label class="option type-hub-body">
          <input type="checkbox" bind:checked={form.gst} disabled={!canEdit || locked} />
          <span>{t("hub.settings.tax.gst")}</span>
        </label>
        {#if form.gst}{@render text("gstin")}{/if}
      {:else}
        {@render text("label")}
      {/if}

      <fieldset class="field group">
        <legend class="label type-hub-label">{t("hub.settings.tax.inclusive")}</legend>
        <div class="options">
          {#each [["true", "hub.settings.tax.inclusive_yes"], ["false", "hub.settings.tax.inclusive_no"]] as const as [value, label] (value)}
            <label class="option type-hub-body">
              <input
                type="radio"
                name="inclusive"
                {value}
                bind:group={form.inclusive}
                disabled={!canEdit || locked}
              />
              <span>{t(label)}</span>
            </label>
          {/each}
        </div>
      </fieldset>

      {#if india}
        {@render text("product", "decimal")}
        {@render text("play", "decimal")}
        {@render text("sac")}
      {:else}
        {@render text("rate", "decimal")}
      {/if}

      <div class="field">
        <p class="label type-hub-label" id="tax-prefix-label">{t("hub.settings.tax.prefix")}</p>
        <output class="type-hub-title" aria-labelledby="tax-prefix-label" data-testid="tax-prefix"
          >{code}</output
        >
        {#if codeLocked}
          <p class="label type-hub-label" data-testid="tax-prefix-locked">
            {t("hub.settings.tax.prefix_locked")}
          </p>
        {/if}
      </div>

      <div class="actions">
        <span class="save" bind:this={saveWrap}>
          <Button
            label="hub.settings.save"
            type="submit"
            disabled={!canEdit || locked || loading || !changed}
            disabledReason={approving
              ? "hub.settings.branch.needs_approval"
              : saving
                ? "hub.settings.saving"
                : loading
                  ? "hub.settings.loading"
                  : "hub.settings.branch.no_changes"}
          />
        </span>
        {#if needsApproval && !approving}
          <p class="note type-hub-label" data-testid="tax-needs-approval">
            {t("hub.settings.branch.needs_approval")}
          </p>
        {/if}
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="tax-status"
        >
          {#if status}
            {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{t(status.message)}</span>
          {/if}
        </p>
      </div>
      {#if approving}
        <div class="approval" data-testid="tax-approval">
          <InlinePin
            message={approvalPrompt}
            onsubmit={approve}
            oncancel={() => (approving = false)}
            returnFocusTo={saveButton}
          />
        </div>
      {/if}
    </form>
  {/if}
</section>

<style>
  .tax {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  .intro,
  .note,
  .error,
  .status {
    margin: 0;
  }

  .intro {
    color: var(--text-muted);
  }

  .form {
    display: grid;
    gap: var(--space-4);
    max-width: 32rem;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }

  .label {
    margin: 0;
    padding: 0;
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .control {
    --ui-bg: var(--text-field-background);
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    max-width: 100%;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
    font-size: var(--type-web-body-size);
  }

  .control:focus-visible {
    border-width: 2px;
    border-color: var(--accent);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .control.invalid {
    border-width: 2px;
    border-color: var(--danger);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .control:disabled,
  .option:has(input:disabled) {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .options {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-5);
  }

  .option {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
    color: var(--text);
    cursor: pointer;
  }

  .option input {
    width: var(--space-5);
    height: var(--space-5);
    margin: 0;
    accent-color: var(--accent);
  }

  .note,
  .status {
    color: var(--text);
  }

  .error,
  .status.refusal {
    color: var(--text-field-error-text);
  }

  .error,
  .status {
    display: flex;
    gap: var(--space-2);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }

  .approval {
    display: grid;
    gap: var(--space-2);
  }
</style>
