<script lang="ts">
  import { Button, t } from "@arena/ui";
  import { focusFirstInvalid } from "@arena/ui";
  import {
    CURRENCIES,
    TIME_ZONES,
    currencyOf,
    formatBusinessDay,
    formatMoney,
  } from "@arena/ui/format";
  import type { SessionStaff } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    hasPermission,
    loadSettings,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { branchLocale } from "../../../lib/locale.svelte";
  import { SNAPSHOT_DEPENDENCY } from "../../../lib/snapshot";

  type Field = "currency" | "symbol_position" | "number_format" | "tz" | "day_start" | "fy_start";

  /** The fields in form order (Save submits changed ones in this order) and their keys. */
  const FIELDS: readonly Field[] = [
    "currency",
    "symbol_position",
    "number_format",
    "tz",
    "day_start",
    "fy_start",
  ];
  const KEYS: Record<Field, string> = {
    currency: "branch.currency",
    symbol_position: "branch.symbol_position",
    number_format: "branch.number_format",
    tz: "branch.tz",
    day_start: "branch.day_start_min",
    fy_start: "branch.fy_start_month",
  };
  const INVALID: Record<Field, MessageKey> = {
    currency: "hub.settings.branch.invalid.currency",
    symbol_position: "hub.settings.branch.invalid.symbol_position",
    number_format: "hub.settings.branch.invalid.number_format",
    tz: "hub.settings.branch.invalid.tz",
    day_start: "hub.settings.branch.invalid.day_start",
    fy_start: "hub.settings.branch.invalid.fy_start",
  };
  const SYMBOL_POSITIONS: readonly (readonly [string, MessageKey])[] = [
    ["before", "hub.settings.branch.symbol_before"],
    ["after", "hub.settings.branch.symbol_after"],
  ];
  const NUMBER_FORMATS: readonly (readonly [string, MessageKey])[] = [
    ["indian", "hub.settings.branch.number_indian"],
    ["international", "hub.settings.branch.number_international"],
  ];
  const LABELS: Record<Field, MessageKey> = {
    currency: "hub.settings.branch.currency",
    symbol_position: "hub.settings.branch.symbol_position",
    number_format: "hub.settings.branch.number_format",
    tz: "hub.settings.branch.tz",
    day_start: "hub.settings.branch.day_start",
    fy_start: "hub.settings.branch.fy_start",
  };
  /** The live preview's sample, 123456.78, in minor units of a currency with `exponent`. */
  const sampleMinor = (exponent: number): bigint =>
    exponent >= 2
      ? 12345678n * 10n ** BigInt(exponent - 2)
      : 12345678n / 10n ** BigInt(2 - exponent);

  const blank = (): Record<Field, string> => ({
    currency: "",
    symbol_position: "",
    number_format: "",
    tz: "",
    day_start: "",
    fy_start: "",
  });

  /** What the Hub last reported, with each row's `rev` (an edit's `base_rev`). */
  let server: Partial<Record<Field, { value: string; rev: number }>> = $state({});
  /** The form, in the settings' own text form (`day_start` in minutes). */
  let form: Record<Field, string> = $state(blank());
  let errors: Partial<Record<Field, MessageKey>> = $state({});
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let status: {
    tone: "ok" | "refusal";
    message: MessageKey;
    params?: Record<string, string>;
  } | null = $state(null);
  /** After a STALE save: each refused field's value now on the Hub (the typed value is kept). */
  let hubValues: Partial<Record<Field, string>> = $state({});
  let formEl: HTMLFormElement | undefined = $state();
  /** One idempotency key per field edit, kept while that same edit is retried. */
  const pendingKeys = new Map<Field, { value: string; key: string }>();

  const staff = $derived.by(() => {
    const session = (page.data as { session?: { state: string; staff?: SessionStaff } }).session;
    return session?.state === "signed_in" ? (session.staff ?? null) : null;
  });
  const canEdit = $derived(hasPermission(staff, "settings.edit"));
  const changed = $derived(
    FIELDS.filter((f) => server[f] !== undefined && form[f] !== server[f]?.value),
  );
  const pending = $derived(branchLocale.current?.pending ?? null);

  const preview = $derived.by(() => {
    const c = currencyOf(form.currency);
    if (!c) return "";
    return formatMoney(sampleMinor(c.exponent), {
      exponent: c.exponent,
      symbol: c.symbol,
      symbol_position: form.symbol_position === "after" ? "after" : "before",
      number_format: form.number_format === "international" ? "international" : "indian",
    });
  });

  /** The Hub's zone list (from chrono-tz), with the current value kept even if it is not listed. */
  const zones = $derived(
    form.tz && !TIME_ZONES.includes(form.tz) ? [form.tz, ...TIME_ZONES] : TIME_ZONES,
  );

  const monthName = (m: number) =>
    new Intl.DateTimeFormat(undefined, { month: "long", timeZone: "UTC" }).format(
      Date.UTC(2000, m - 1, 1),
    );
  const MONTHS = Array.from({ length: 12 }, (_, i) => i + 1);

  /** Minutes after midnight → `HH:MM` (anything else is shown empty). */
  function toClock(minutes: string): string {
    const n = Number(minutes);
    if (minutes === "" || !Number.isInteger(n) || n < 0 || n > 1439) return "";
    return `${String(Math.floor(n / 60)).padStart(2, "0")}:${String(n % 60).padStart(2, "0")}`; // i18n-ignore (HH:MM)
  }

  /** `HH:MM` → minutes after midnight, as text (empty when it is not a time). */
  function toMinutes(clock: string): string {
    const m = /^(\d{2}):(\d{2})$/.exec(clock);
    if (!m) return "";
    return String(Number(m[1]) * 60 + Number(m[2]));
  }

  /** A field's value as the form shows it (for "Now on the Hub: …"). */
  function shown(f: Field, value: string): string {
    if (f === "day_start") return toClock(value) || value;
    if (f === "fy_start")
      return Number(value) >= 1 && Number(value) <= 12 ? monthName(Number(value)) : value;
    if (f === "symbol_position" || f === "number_format") {
      const list = f === "symbol_position" ? SYMBOL_POSITIONS : NUMBER_FORMATS;
      const hit = list.find(([v]) => v === value);
      return hit ? t(hit[1]) : value;
    }
    return value;
  }

  async function signedOut() {
    await invalidate(SESSION_DEPENDENCY);
  }

  /**
   * Reads the settings. `keep` lists the fields whose typed value stays in the form (the
   * others show the Hub's value).
   */
  async function load(keep: readonly Field[] = []): Promise<boolean> {
    loading = true;
    try {
      const result = await loadSettings();
      if (result.kind === "signed_out") {
        await signedOut();
        return false;
      }
      const next: Partial<Record<Field, { value: string; rev: number }>> = {};
      if (result.kind === "ok") {
        for (const f of FIELDS) {
          const entry = result.settings.find((s) => s.key === KEYS[f]);
          if (entry) next[f] = { value: entry.value, rev: entry.rev };
        }
      }
      if (FIELDS.some((f) => next[f] === undefined)) {
        loadFailed = true;
        return false;
      }
      loadFailed = false;
      server = next;
      for (const f of FIELDS) {
        if (!keep.includes(f)) form[f] = next[f]!.value;
      }
      return true;
    } finally {
      loading = false;
    }
  }

  async function save() {
    if (!canEdit || saving || loading || changed.length === 0) return;
    saving = true;
    status = null;
    errors = {};
    hubValues = {};
    const edits = [...changed];
    const saved: Field[] = [];
    let failedAt: Field | null = null;
    let stale = false;
    try {
      for (const f of edits) {
        const value = form[f];
        const base = server[f];
        if (!base) continue;
        let idem = pendingKeys.get(f);
        if (idem?.value !== value) {
          idem = { value, key: newIdempotencyKey() };
          pendingKeys.set(f, idem);
        }
        const outcome = await submitIntent(
          { kind: "settings.set", key: KEYS[f], value },
          idem.key,
          base.rev,
        );
        if (outcome.kind === "signed_out") return signedOut();
        if (outcome.kind === "applied") {
          pendingKeys.delete(f);
          saved.push(f);
          continue;
        }
        failedAt = f;
        if (outcome.kind === "error" && outcome.error.code === "VALIDATION_FAILED") {
          pendingKeys.delete(f);
          errors = { [f]: INVALID[f] };
        } else if (outcome.kind === "error" && outcome.error.code === "STALE") {
          // Someone else saved this field: keep the typed value, mark the field and show theirs.
          pendingKeys.delete(f);
          stale = true;
          errors = { [f]: "hub.settings.branch.stale" };
          status = { tone: "refusal", message: "hub.settings.branch.stale" };
          void invalidate(SNAPSHOT_DEPENDENCY);
        } else if (outcome.kind === "error" && outcome.error.code === "PERMISSION_DENIED") {
          pendingKeys.delete(f);
          status = { tone: "refusal", message: "hub.settings.branch.manager_only" };
        } else {
          status = { tone: "refusal", message: "hub.settings.failed" };
        }
        break;
      }
      // The Hub's revs, not guesses. Edits not yet saved (the refused one included) stay in the
      // form.
      const unsaved = failedAt === null ? [] : edits.slice(edits.indexOf(failedAt));
      await load(unsaved);
      if (stale && failedAt && server[failedAt]) {
        hubValues = { [failedAt]: shown(failedAt, server[failedAt]!.value) };
      }
      if (failedAt === null) {
        status = { tone: "ok", message: "hub.settings.saved" };
      } else if (saved.length > 0) {
        // Some fields went through before the refusal: say which.
        status = {
          tone: "refusal",
          message: "hub.settings.branch.partial",
          params: {
            saved: saved.map((s) => t(LABELS[s])).join(", "),
            failed: t(LABELS[failedAt]),
          },
        };
      }
    } finally {
      saving = false;
    }
    if (Object.keys(errors).length) {
      await tick();
      focusFirstInvalid(formEl);
    }
  }

  // Another window (or this one) changed settings: refresh the untouched fields, keep edits.
  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      if (!saving) void load([...changed]);
    });
  });

  onMount(() => {
    void load();
  });

  const errorId = (f: Field) => `branch-${f}-error`; // i18n-ignore (an element id)
  const describedBy = (f: Field, extra?: string) =>
    [errors[f] ? errorId(f) : null, extra].filter(Boolean).join(" ") || undefined;
</script>

{#snippet fieldError(f: Field)}
  <p class="error type-hub-label" id={errorId(f)} aria-live="polite">
    {#if errors[f]}<span aria-hidden="true">⚠</span>
      <span>{t(errors[f]!)}</span>{/if}
  </p>
  {#if hubValues[f] !== undefined}
    <p class="note type-hub-label" data-testid="branch-{f}-hub-value">
      {t("hub.settings.server_value", { value: hubValues[f]! })}
    </p>
  {/if}
{/snippet}

<section class="branch" aria-labelledby="settings-branch-title">
  <h2 class="type-hub-title" id="settings-branch-title">{t("hub.settings.branch.title")}</h2>
  <p class="intro type-hub-body">{t("hub.settings.branch.intro")}</p>

  {#if loadFailed}
    <div class="actions" data-testid="branch-load-failed">
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
      data-testid="settings-branch"
      bind:this={formEl}
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div class="field">
        <label class="label type-hub-label" for="branch-currency"
          >{t("hub.settings.branch.currency")}</label
        >
        <select
          id="branch-currency"
          class="control ui-interactive"
          class:invalid={!!errors.currency}
          bind:value={form.currency}
          disabled={!canEdit || saving}
          aria-invalid={errors.currency ? "true" : undefined}
          aria-describedby={describedBy("currency")}
        >
          {#if form.currency && !currencyOf(form.currency)}
            <option value={form.currency}>{form.currency}</option>
          {/if}
          {#each CURRENCIES as c (c.code)}
            <option value={c.code}
              >{t("hub.settings.branch.currency_option", {
                code: c.code,
                symbol: c.symbol,
              })}</option
            >
          {/each}
        </select>
        {@render fieldError("currency")}
      </div>

      <fieldset class="field group" aria-describedby={describedBy("symbol_position")}>
        <legend class="label type-hub-label">{t("hub.settings.branch.symbol_position")}</legend>
        <div class="options">
          {#each SYMBOL_POSITIONS as [value, label] (value)}
            <label class="option type-hub-body">
              <input
                type="radio"
                name="symbol_position"
                {value}
                bind:group={form.symbol_position}
                disabled={!canEdit || saving}
              />
              <span>{t(label)}</span>
            </label>
          {/each}
        </div>
        {@render fieldError("symbol_position")}
      </fieldset>

      <fieldset class="field group" aria-describedby={describedBy("number_format")}>
        <legend class="label type-hub-label">{t("hub.settings.branch.number_format")}</legend>
        <div class="options">
          {#each NUMBER_FORMATS as [value, label] (value)}
            <label class="option type-hub-body">
              <input
                type="radio"
                name="number_format"
                {value}
                bind:group={form.number_format}
                disabled={!canEdit || saving}
              />
              <span class="tabular-nums">{t(label)}</span>
            </label>
          {/each}
        </div>
        {@render fieldError("number_format")}
      </fieldset>

      <div class="field">
        <label class="label type-hub-label" for="branch-tz">{t("hub.settings.branch.tz")}</label>
        {#if zones.length}
          <select
            id="branch-tz"
            class="control ui-interactive"
            class:invalid={!!errors.tz}
            bind:value={form.tz}
            disabled={!canEdit || saving}
            aria-invalid={errors.tz ? "true" : undefined}
            aria-describedby={describedBy("tz", pending?.tz ? "branch-tz-pending" : undefined)}
          >
            {#each zones as zone (zone)}
              <option value={zone}>{zone}</option>
            {/each}
          </select>
        {:else}
          <input
            id="branch-tz"
            class="control ui-interactive"
            class:invalid={!!errors.tz}
            type="text"
            autocomplete="off"
            spellcheck="false"
            bind:value={form.tz}
            disabled={!canEdit || saving}
            aria-invalid={errors.tz ? "true" : undefined}
            aria-describedby={describedBy("tz", pending?.tz ? "branch-tz-pending" : undefined)}
          />
        {/if}
        {#if pending?.tz}
          <p class="note type-hub-label" id="branch-tz-pending" data-testid="branch-tz-pending">
            {t("hub.settings.branch.takes_effect", {
              date: formatBusinessDay(pending.tz.effective_from),
            })}
          </p>
        {/if}
        {@render fieldError("tz")}
      </div>

      <div class="field">
        <label class="label type-hub-label" for="branch-day-start"
          >{t("hub.settings.branch.day_start")}</label
        >
        <input
          id="branch-day-start"
          class="control ui-interactive tabular-nums"
          class:invalid={!!errors.day_start}
          type="time"
          step="60"
          bind:value={() => toClock(form.day_start), (v) => (form.day_start = toMinutes(v))}
          disabled={!canEdit || saving}
          aria-invalid={errors.day_start ? "true" : undefined}
          aria-describedby={describedBy(
            "day_start",
            pending?.day_start_min ? "branch-day-start-pending" : undefined,
          )}
        />
        {#if pending?.day_start_min}
          <p
            class="note type-hub-label"
            id="branch-day-start-pending"
            data-testid="branch-day-start-pending"
          >
            {t("hub.settings.branch.takes_effect", {
              date: formatBusinessDay(pending.day_start_min.effective_from),
            })}
          </p>
        {/if}
        {@render fieldError("day_start")}
      </div>

      <div class="field">
        <label class="label type-hub-label" for="branch-fy-start"
          >{t("hub.settings.branch.fy_start")}</label
        >
        <select
          id="branch-fy-start"
          class="control ui-interactive"
          class:invalid={!!errors.fy_start}
          bind:value={form.fy_start}
          disabled={!canEdit || saving}
          aria-invalid={errors.fy_start ? "true" : undefined}
          aria-describedby={describedBy(
            "fy_start",
            pending?.fy_start_month ? "branch-fy-start-pending" : undefined,
          )}
        >
          {#each MONTHS as m (m)}
            <option value={String(m)}>{monthName(m)}</option>
          {/each}
        </select>
        {#if pending?.fy_start_month}
          <p
            class="note type-hub-label"
            id="branch-fy-start-pending"
            data-testid="branch-fy-start-pending"
          >
            {t("hub.settings.branch.takes_effect", {
              date: formatBusinessDay(pending.fy_start_month.effective_from),
            })}
          </p>
        {/if}
        {@render fieldError("fy_start")}
      </div>

      <div class="preview" data-testid="branch-preview">
        <p class="label type-hub-label" id="branch-preview-label">
          {t("hub.settings.branch.preview")}
        </p>
        <output
          class="amount tabular-nums type-hub-title"
          aria-labelledby="branch-preview-label"
          data-testid="branch-preview-amount">{preview}</output
        >
      </div>

      <div class="actions">
        <Button
          label="hub.settings.save"
          type="submit"
          disabled={!canEdit || saving || loading || changed.length === 0}
          disabledReason={!canEdit
            ? "hub.settings.branch.manager_only"
            : saving
              ? "hub.settings.saving"
              : loading
                ? "hub.settings.loading"
                : "hub.settings.branch.no_changes"}
        />
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="branch-status"
        >
          {#if status}
            {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{t(status.message, status.params)}</span>
          {/if}
        </p>
      </div>
    </form>
  {/if}
</section>

<style>
  .branch {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  .intro {
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

  .note {
    margin: 0;
    color: var(--text);
  }

  .error {
    display: flex;
    gap: var(--space-2);
    margin: 0;
    color: var(--text-field-error-text);
  }

  .preview {
    display: grid;
    gap: var(--space-1);
  }

  .preview p {
    margin: 0;
  }

  .amount {
    color: var(--text);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }

  .status {
    display: flex;
    gap: var(--space-2);
    margin: 0;
    min-height: 1lh;
    color: var(--text);
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }
</style>
