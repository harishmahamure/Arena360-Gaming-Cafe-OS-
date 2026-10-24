<script lang="ts">
  import { Button, focusFirstInvalid, t } from "@arena/ui";
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
  } from "../../../../lib/auth";
  import { branchLocale } from "../../../../lib/locale.svelte";
  import {
    DAYS_PER_WEEK,
    type Day,
    KEY_OPENING_HOURS,
    KEY_OPENING_HOURS_SET,
    dayOfField,
    parseClock,
    parseWeek,
    sessionStaff,
    toClock,
    weekJson,
  } from "../../../../lib/operating";

  type Row = { closed: boolean; open: string; close: string };
  type RowErrors = { open?: MessageKey; close?: MessageKey; row?: MessageKey };

  const DAY_LABELS: MessageKey[] = [
    "hub.settings.hours.day.0",
    "hub.settings.hours.day.1",
    "hub.settings.hours.day.2",
    "hub.settings.hours.day.3",
    "hub.settings.hours.day.4",
    "hub.settings.hours.day.5",
    "hub.settings.hours.day.6",
  ];
  const INDEXES = Array.from({ length: DAYS_PER_WEEK }, (_, i) => i);

  const toRow = (d: Day): Row =>
    "closed" in d
      ? { closed: true, open: "", close: "" }
      : { closed: false, open: toClock(d.open_min), close: toClock(d.close_min) };

  let server: { value: string; rev: number } | null = $state(null);
  let hoursSet = $state(true);
  let rows: Row[] = $state(INDEXES.map(() => ({ closed: true, open: "", close: "" })));
  let errors: RowErrors[] = $state(INDEXES.map(() => ({})));
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let formEl: HTMLFormElement | undefined = $state();
  let pendingKey: { value: string; key: string } | null = null;
  let loadSeq = 0;

  const staff = $derived(sessionStaff(page.data));
  const canEdit = $derived(hasPermission(staff, "settings.edit"));
  const serverRows = $derived.by(() => (server ? (parseWeek(server.value) ?? []).map(toRow) : []));
  const dirty = $derived(
    serverRows.length === DAYS_PER_WEEK &&
      rows.some(
        (r, i) =>
          r.closed !== serverRows[i].closed ||
          (!r.closed && (r.open !== serverRows[i].open || r.close !== serverRows[i].close)),
      ),
  );
  const hasOpenDay = $derived(rows.some((r) => !r.closed));
  const canSave = $derived(
    canEdit && !saving && !loading && server !== null && hasOpenDay && (dirty || !hoursSet),
  );

  function wraps(r: Row): boolean {
    if (r.closed) return false;
    const open = parseClock(r.open);
    const close = parseClock(r.close);
    return open !== null && close !== null && close < open;
  }

  async function load(keepEdits = false) {
    const seq = ++loadSeq;
    loading = true;
    try {
      const result = await loadSettings();
      if (seq !== loadSeq) return;
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const entry =
        result.kind === "ok" ? result.settings.find((s) => s.key === KEY_OPENING_HOURS) : undefined;
      const week = entry ? parseWeek(entry.value) : null;
      if (!entry || !week) {
        loadFailed = true;
        return;
      }
      loadFailed = false;
      server = { value: entry.value, rev: entry.rev };
      const flag =
        result.kind === "ok"
          ? result.settings.find((s) => s.key === KEY_OPENING_HOURS_SET)
          : undefined;
      hoursSet = flag?.value === "true";
      if (!keepEdits) rows = week.map(toRow);
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  function collect(): Day[] | null {
    const next: RowErrors[] = INDEXES.map(() => ({}));
    const days: Day[] = [];
    rows.forEach((r, i) => {
      if (r.closed) {
        days.push({ closed: true });
        return;
      }
      const open = parseClock(r.open);
      const close = parseClock(r.close);
      if (open === null) next[i].open = "hub.settings.hours.invalid.time";
      if (close === null) next[i].close = "hub.settings.hours.invalid.time";
      if (open !== null && close !== null) {
        if (open === close) next[i].row = "hub.settings.hours.invalid.same";
        else days.push({ open_min: open, close_min: close });
      }
    });
    errors = next;
    return days.length === DAYS_PER_WEEK ? days : null;
  }

  async function save() {
    if (!canSave) return;
    status = null;
    const days = collect();
    if (!days) {
      await tick();
      focusFirstInvalid(formEl);
      return;
    }
    const value = weekJson(days);
    if (pendingKey?.value !== value) pendingKey = { value, key: newIdempotencyKey() };
    saving = true;
    try {
      const outcome = await submitIntent(
        { kind: "settings.set", key: KEY_OPENING_HOURS, value },
        pendingKey.key,
        server!.rev,
      );
      if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (outcome.kind === "applied") {
        pendingKey = null;
        status = { tone: "ok", message: "hub.settings.saved" };
        await load();
        return;
      }
      if (outcome.kind === "error") {
        const code = outcome.error.code;
        if (code === "VALIDATION_FAILED") {
          pendingKey = null;
          const day = dayOfField(outcome.error.field);
          if (day !== null) {
            errors = INDEXES.map((i) =>
              i === day ? { row: "hub.settings.hours.invalid.day" } : {},
            );
          }
          status = { tone: "refusal", message: "hub.settings.hours.invalid.week" };
        } else if (code === "STALE") {
          pendingKey = null;
          status = { tone: "refusal", message: "hub.settings.branch.stale" };
          await load(true);
        } else if (code === "PERMISSION_DENIED") {
          pendingKey = null;
          status = { tone: "refusal", message: "hub.settings.branch.manager_only" };
        } else {
          status = { tone: "refusal", message: "hub.settings.failed" };
        }
      } else {
        status = { tone: "refusal", message: "hub.settings.failed" };
      }
    } finally {
      saving = false;
    }
    if (errors.some((e) => e.open || e.close || e.row)) {
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
      if (!saving && (keys.includes(KEY_OPENING_HOURS) || keys.includes(KEY_OPENING_HOURS_SET)))
        void load(dirty);
    });
  });

  onMount(() => {
    void load();
  });

  const fieldId = (i: number, part: string) => ["hours", String(i), part].join("-");
  const describedBy = (i: number, part: "open" | "close") =>
    [
      fieldId(i, "hint"),
      errors[i][part] ? fieldId(i, part + "-error") : null,
      errors[i].row ? fieldId(i, "row-error") : null,
      wraps(rows[i]) ? fieldId(i, "next") : null,
    ]
      .filter(Boolean)
      .join(" ");
</script>

<section class="hours" aria-labelledby="settings-hours-title">
  <h2 class="type-hub-title" id="settings-hours-title">{t("hub.settings.hours.title")}</h2>
  <p class="intro type-hub-body">{t("hub.settings.hours.intro")}</p>
  {#if !loading && !loadFailed && !hoursSet}
    <p class="note type-hub-label" data-testid="hours-not-set">{t("hub.settings.hours.not_set")}</p>
  {/if}

  {#if loadFailed}
    <div class="actions" data-testid="hours-load-failed">
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
      data-testid="settings-hours"
      bind:this={formEl}
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      {#each INDEXES as i (i)}
        <fieldset class="day" data-testid="hours-day-{i}">
          <legend class="label type-hub-label">{t(DAY_LABELS[i])}</legend>
          <div class="row">
            <label class="option type-hub-body">
              <input
                type="checkbox"
                bind:checked={rows[i].closed}
                disabled={!canEdit || saving}
                data-testid="hours-closed-{i}"
              />
              <span>{t("hub.settings.hours.closed")}</span>
            </label>
            {#if !rows[i].closed}
              <div class="time">
                <label class="type-hub-label" for={fieldId(i, "open")}
                  >{t("hub.settings.hours.opens")}</label
                >
                <input
                  id={fieldId(i, "open")}
                  class="control ui-interactive tabular-nums"
                  class:invalid={!!errors[i].open || !!errors[i].row}
                  type="text"
                  inputmode="numeric"
                  autocomplete="off"
                  maxlength="5"
                  bind:value={rows[i].open}
                  disabled={!canEdit || saving}
                  aria-invalid={errors[i].open || errors[i].row ? "true" : undefined}
                  aria-describedby={describedBy(i, "open")}
                />
              </div>
              <div class="time">
                <label class="type-hub-label" for={fieldId(i, "close")}
                  >{t("hub.settings.hours.closes")}</label
                >
                <input
                  id={fieldId(i, "close")}
                  class="control ui-interactive tabular-nums"
                  class:invalid={!!errors[i].close || !!errors[i].row}
                  type="text"
                  inputmode="numeric"
                  autocomplete="off"
                  maxlength="5"
                  bind:value={rows[i].close}
                  disabled={!canEdit || saving}
                  aria-invalid={errors[i].close || errors[i].row ? "true" : undefined}
                  aria-describedby={describedBy(i, "close")}
                />
              </div>
            {/if}
          </div>
          {#if !rows[i].closed}
            <p class="hint type-hub-label" id={fieldId(i, "hint")}>
              {t("hub.settings.hours.time_hint")}
            </p>
          {/if}
          {#if wraps(rows[i])}
            <p class="note type-hub-label" id={fieldId(i, "next")} data-testid="hours-next-day-{i}">
              {t("hub.settings.hours.next_day")}
            </p>
          {/if}
          {#each ["open", "close", "row"] as const as part (part)}
            {#if errors[i][part]}
              <p
                class="error type-hub-label"
                id={fieldId(i, part + "-error")}
                data-testid="hours-error-{i}-{part}"
              >
                <span aria-hidden="true">⚠</span>
                <span>{t(errors[i][part]!)}</span>
              </p>
            {/if}
          {/each}
        </fieldset>
      {/each}

      <div class="actions">
        <Button
          label="hub.settings.save"
          type="submit"
          disabled={!canSave}
          disabledReason={!canEdit
            ? "hub.settings.branch.manager_only"
            : saving
              ? "hub.settings.saving"
              : loading
                ? "hub.settings.loading"
                : !hasOpenDay
                  ? "hub.settings.hours.no_open_day"
                  : "hub.settings.branch.no_changes"}
        />
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="hours-status"
        >
          {#if status}
            {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{t(status.message)}</span>
          {/if}
        </p>
      </div>
    </form>
  {/if}
</section>

<style>
  .hours {
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
    gap: var(--space-3);
    max-width: 40rem;
  }

  .day {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: var(--space-3) 0;
    border: 0;
    border-bottom: 1px solid var(--border-strong);
  }

  .label {
    padding: 0;
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-3) var(--space-5);
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

  .time {
    display: grid;
    gap: var(--space-1);
    color: var(--text);
  }

  .control {
    --ui-bg: var(--text-field-background);
    width: 6.5rem;
    min-height: var(--space-touch-min);
    box-sizing: border-box;
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

  .hint {
    margin: 0;
    color: var(--text-muted);
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
