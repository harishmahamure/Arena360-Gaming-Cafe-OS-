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
  import { FIELDS, KEYS, MODES, validField, type Field } from "../../../../lib/billing";
  import { branchLocale } from "../../../../lib/locale.svelte";
  import { sessionStaff } from "../../../../lib/operating";

  const INVALID: Record<Field, MessageKey> = {
    rounding: "hub.settings.billing.invalid.rounding",
    minimum: "hub.settings.billing.invalid.minimum",
    grace: "hub.settings.billing.invalid.grace",
    mode: "hub.settings.billing.invalid.mode",
  };
  const NUMBERS = [
    [
      "rounding",
      "hub.settings.billing.rounding",
      "hub.settings.billing.rounding_hint",
      "hub.settings.billing.rounding_help",
    ],
    [
      "minimum",
      "hub.settings.billing.minimum",
      "hub.settings.billing.minimum_hint",
      "hub.settings.billing.minimum_help",
    ],
    [
      "grace",
      "hub.settings.billing.grace",
      "hub.settings.billing.grace_hint",
      "hub.settings.billing.grace_help",
    ],
  ] as const satisfies readonly (readonly [Field, MessageKey, MessageKey, MessageKey])[];
  const MODE_LABELS: Record<(typeof MODES)[number], MessageKey> = {
    clean_up: "hub.settings.billing.time_up_clean_up",
    staff_handles: "hub.settings.billing.time_up_staff_handles",
  };

  let server: Partial<Record<Field, { value: string; rev: number }>> = $state({});
  let form: Record<Field, string> = $state({
    rounding: "15",
    minimum: "30",
    grace: "2",
    mode: "clean_up",
  });
  let errors: Partial<Record<Field, MessageKey>> = $state({});
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let formEl: HTMLFormElement | undefined = $state();
  let saveWrap: HTMLElement | undefined = $state();
  const pendingKeys = new Map<Field, { value: string; key: string }>();
  let loadSeq = 0;

  const staff = $derived(sessionStaff(page.data));
  const canEdit = $derived(staff !== null);
  const needsApproval = $derived(!hasPermission(staff, "settings.edit"));
  const locked = $derived(saving || approving);
  const saveButton = $derived(saveWrap?.querySelector("button") ?? null);
  const changed = $derived(
    FIELDS.filter((f) => server[f] !== undefined && form[f] !== server[f]?.value),
  );

  async function load(keep: readonly Field[] = []) {
    const seq = ++loadSeq;
    loading = true;
    try {
      const result = await loadSettings();
      if (seq !== loadSeq) return;
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const next: Partial<Record<Field, { value: string; rev: number }>> = {};
      if (result.kind === "ok") {
        for (const f of FIELDS) {
          const entry = result.settings.find((s) => s.key === KEYS[f]);
          if (entry) next[f] = { value: entry.value, rev: entry.rev };
        }
      }
      if (FIELDS.some((f) => next[f] === undefined)) {
        loadFailed = true;
        return;
      }
      loadFailed = false;
      server = next;
      for (const f of FIELDS) if (!keep.includes(f)) form[f] = next[f]!.value;
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  async function approve(pin: string) {
    const items = changed.map((f) => ({ intent_kind: "settings.set" as const, key: KEYS[f] }));
    const outcome = await elevate(items, pin);
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
    if (!canEdit || saving || loading || changed.length === 0) {
      if (ticket) void dropElevation(ticket);
      return;
    }
    const bad = changed.filter((f) => !validField(f, form[f]));
    if (bad.length) {
      errors = Object.fromEntries(bad.map((f) => [f, INVALID[f]]));
      status = null;
      if (ticket) void dropElevation(ticket);
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
    const edits = [...changed];
    let failedAt: Field | null = null;
    let again = false;
    let stale = false;
    try {
      for (const f of edits) {
        const value = form[f];
        let idem = pendingKeys.get(f);
        if (idem?.value !== value) {
          idem = { value, key: newIdempotencyKey() };
          pendingKeys.set(f, idem);
        }
        const outcome = await submitIntent(
          { kind: "settings.set", key: KEYS[f], value },
          idem.key,
          server[f]!.rev,
          { elevationTicket: ticket },
        );
        if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
        if (outcome.kind === "applied") {
          pendingKeys.delete(f);
          continue;
        }
        failedAt = f;
        const code = outcome.kind === "error" ? outcome.error.code : null;
        if (code === "VALIDATION_FAILED") {
          pendingKeys.delete(f);
          errors = { [f]: INVALID[f] };
        } else if (code === "STALE") {
          pendingKeys.delete(f);
          stale = true;
          errors = { [f]: "hub.settings.branch.stale" };
          status = { tone: "refusal", message: "hub.settings.branch.stale" };
        } else if (code === "PIN_REQUIRED") {
          again = true;
        } else if (code === "PERMISSION_DENIED") {
          pendingKeys.delete(f);
          status = { tone: "refusal", message: "hub.settings.branch.manager_only" };
        } else {
          status = { tone: "refusal", message: "hub.settings.failed" };
        }
        break;
      }
      await load(failedAt === null || stale ? [] : edits.slice(edits.indexOf(failedAt)));
      if (failedAt === null) status = { tone: "ok", message: "hub.settings.saved" };
    } finally {
      saving = false;
      if (ticket) void dropElevation(ticket);
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
      const keys = branchLocale.changedKeys;
      if (!locked && FIELDS.some((f) => keys.includes(KEYS[f]))) void load([...changed]);
    });
  });

  onMount(() => {
    void load();
  });

  const describedBy = (f: Field, ...extra: string[]) =>
    [errors[f] ? "billing-" + f + "-error" : null, ...extra].filter(Boolean).join(" ") || undefined;
</script>

{#snippet fieldError(f: Field)}
  <p class="error type-hub-label" id={"billing-" + f + "-error"} aria-live="polite">
    {#if errors[f]}<span aria-hidden="true">⚠</span>
      <span>{t(errors[f]!)}</span>{/if}
  </p>
{/snippet}

<section class="billing" aria-labelledby="settings-billing-title">
  <h2 class="type-hub-title" id="settings-billing-title">{t("hub.settings.billing.title")}</h2>
  <p class="intro type-hub-body">{t("hub.settings.billing.intro")}</p>

  {#if loadFailed}
    <div class="actions" data-testid="billing-load-failed">
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
      data-testid="settings-billing"
      novalidate
      bind:this={formEl}
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      {#each NUMBERS as [f, label, hint, help] (f)}
        <div class="field">
          <label class="label type-hub-label" for={"billing-" + f}>{t(label)}</label>
          <input
            id={"billing-" + f}
            class="control ui-interactive tabular-nums"
            class:invalid={!!errors[f]}
            type="text"
            inputmode="numeric"
            autocomplete="off"
            bind:value={form[f]}
            disabled={!canEdit || locked}
            aria-invalid={errors[f] ? "true" : undefined}
            aria-describedby={describedBy(f, "billing-" + f + "-hint", "billing-" + f + "-help")}
          />
          <p class="hint type-hub-label" id={"billing-" + f + "-hint"}>{t(hint)}</p>
          <p class="hint type-hub-label" id={"billing-" + f + "-help"}>{t(help)}</p>
          {@render fieldError(f)}
        </div>
      {/each}

      <fieldset class="field group" aria-describedby={describedBy("mode", "billing-mode-help")}>
        <legend class="label type-hub-label">{t("hub.settings.billing.time_up")}</legend>
        <div class="options">
          {#each MODES as value (value)}
            <label class="option type-hub-body">
              <input
                type="radio"
                name="time-up-mode"
                {value}
                bind:group={form.mode}
                disabled={!canEdit || locked}
              />
              <span>{t(MODE_LABELS[value])}</span>
            </label>
          {/each}
        </div>
        <p class="hint type-hub-label" id="billing-mode-help">
          {t("hub.settings.billing.time_up_help")}
        </p>
        {@render fieldError("mode")}
      </fieldset>

      <div class="actions">
        <span class="save" bind:this={saveWrap}>
          <Button
            label="hub.settings.save"
            type="submit"
            disabled={!canEdit || locked || loading || changed.length === 0}
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
          <p class="note type-hub-label" data-testid="billing-needs-approval">
            {t("hub.settings.branch.needs_approval")}
          </p>
        {/if}
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="billing-status"
        >
          {#if status}
            {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{t(status.message)}</span>
          {/if}
        </p>
      </div>
      {#if approving}
        <div class="approval" data-testid="billing-approval">
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
  .billing {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  .intro,
  .note,
  .hint,
  .error,
  .status {
    margin: 0;
  }

  .intro,
  .hint {
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
    max-width: 12rem;
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
    min-height: 1lh;
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
