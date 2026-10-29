<script lang="ts">
  // Settings → Games → Booster (Story 6.25): what a PC frees up when a game starts, and how long
  // launches take. The three settings are written by `settings.set`; a change by someone without
  // `settings.edit` asks a manager's PIN in place. Below the form, each PC's games with the
  // median of their measured launch times over the last 7 days.
  import { Button, InlinePin, focusFirstInvalid, t } from "@arena/ui";
  import type { DeviceLaunchTimes } from "@arena/ui/generated";
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
  } from "./auth";
  import { library } from "./catalog.svelte";
  import {
    FIELDS,
    KEYS,
    PRIORITIES,
    PRIORITY_LABELS,
    closeText,
    loadLaunchTimes,
    parsePrograms,
    seconds,
    storedValue,
    validField,
    type Field,
  } from "./booster";
  import { sessionStaff } from "./operating";

  const INVALID: Record<Field, MessageKey> = {
    close: "hub.games.booster.invalid.close",
    priority: "hub.games.booster.invalid.priority",
    power: "hub.games.booster.invalid.power",
  };
  const CLOSE_INVALID = {
    shape: "hub.games.booster.invalid.close",
    duplicate: "hub.games.booster.invalid.close_duplicate",
    too_many: "hub.games.booster.invalid.close_many",
  } as const satisfies Record<string, MessageKey>;

  let server: Partial<Record<Field, { value: string; rev: number }>> = $state({});
  let form: Record<Field, string> = $state({ close: "", priority: "above_normal", power: "false" });
  let errors: Partial<Record<Field, MessageKey>> = $state({});
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let formEl: HTMLFormElement | undefined = $state();
  let saveWrap: HTMLElement | undefined = $state();
  let devices: DeviceLaunchTimes[] = $state([]);
  let timesFailed = $state(false);
  const pendingKeys = new Map<Field, { value: string; key: string }>();
  let loadSeq = 0;
  let timesSeq = 0;

  const staff = $derived(sessionStaff(page.data));
  const canEdit = $derived(staff !== null);
  const needsApproval = $derived(!hasPermission(staff, "settings.edit"));
  const locked = $derived(saving || approving);
  const saveButton = $derived(saveWrap?.querySelector("button") ?? null);
  const changed = $derived(
    FIELDS.filter((f) => server[f] !== undefined && storedValue(f, form[f]) !== server[f]?.value),
  );

  const formText = (f: Field, value: string) => (f === "close" ? closeText(value) : value);

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
      for (const f of FIELDS) if (!keep.includes(f)) form[f] = formText(f, next[f]!.value);
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  async function loadTimes() {
    const seq = ++timesSeq;
    const result = await loadLaunchTimes();
    if (seq !== timesSeq) return;
    if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    timesFailed = result.kind !== "ok";
    if (result.kind === "ok") devices = result.devices;
  }

  /** What is wrong with the form's `f`: the close list says which way it is wrong. */
  function invalidMessage(f: Field): MessageKey {
    if (f !== "close") return INVALID[f];
    const r = parsePrograms(form.close);
    return "problem" in r ? CLOSE_INVALID[r.problem] : INVALID.close;
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
      errors = Object.fromEntries(bad.map((f) => [f, invalidMessage(f)]));
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
        const value = storedValue(f, form[f]);
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
          errors = { [f]: invalidMessage(f) };
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

  // The times are read when the panel opens, and again whenever the library changes.
  $effect(() => {
    if (library.version) untrack(() => void loadTimes());
  });

  onMount(() => {
    void load();
    void loadTimes();
  });

  const describedBy = (f: Field, ...extra: string[]) =>
    [errors[f] ? "booster-" + f + "-error" : null, ...extra].filter(Boolean).join(" ") || undefined;
</script>

{#snippet fieldError(f: Field)}
  <p class="error type-hub-label" id={"booster-" + f + "-error"} aria-live="polite">
    {#if errors[f]}<span aria-hidden="true">⚠</span>
      <span>{t(errors[f]!)}</span>{/if}
  </p>
{/snippet}

<section class="booster" aria-labelledby="games-booster-title" data-testid="booster">
  <h2 class="type-hub-title" id="games-booster-title">{t("hub.games.booster.title")}</h2>
  <p class="intro type-hub-body">{t("hub.games.booster.intro")}</p>

  {#if loadFailed}
    <div class="actions" data-testid="booster-load-failed">
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
      data-testid="booster-form"
      aria-busy={loading}
      novalidate
      bind:this={formEl}
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div class="field">
        <label class="label type-hub-label" for="booster-close"
          >{t("hub.games.booster.close")}</label
        >
        <textarea
          id="booster-close"
          class="control area ui-interactive"
          class:invalid={!!errors.close}
          rows="4"
          autocomplete="off"
          spellcheck="false"
          bind:value={form.close}
          disabled={!canEdit || locked}
          aria-invalid={errors.close ? "true" : undefined}
          aria-describedby={describedBy("close", "booster-close-help")}></textarea>
        <p class="hint type-hub-label" id="booster-close-help">
          {t("hub.games.booster.close_help")}
        </p>
        {@render fieldError("close")}
      </div>

      <fieldset class="field group" aria-describedby={describedBy("priority")}>
        <legend class="label type-hub-label">{t("hub.games.booster.priority")}</legend>
        <div class="options">
          {#each PRIORITIES as value (value)}
            <label class="option type-hub-body">
              <input
                type="radio"
                name="booster-priority"
                {value}
                bind:group={form.priority}
                disabled={!canEdit || locked}
              />
              <span>{t(PRIORITY_LABELS[value])}</span>
            </label>
          {/each}
        </div>
        {@render fieldError("priority")}
      </fieldset>

      <div class="field">
        <label class="option type-hub-body">
          <input
            id="booster-power"
            type="checkbox"
            checked={form.power === "true"}
            onchange={(e) => (form.power = e.currentTarget.checked ? "true" : "false")}
            disabled={!canEdit || locked}
            aria-describedby={describedBy("power", "booster-power-help")}
          />
          <span>{t("hub.games.booster.power")}</span>
        </label>
        <p class="hint type-hub-label" id="booster-power-help">
          {t("hub.games.booster.power_help")}
        </p>
        {@render fieldError("power")}
      </div>

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
          <p class="note type-hub-label" data-testid="booster-needs-approval">
            {t("hub.settings.branch.needs_approval")}
          </p>
        {/if}
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="booster-status"
        >
          {#if status}
            {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{t(status.message)}</span>
          {/if}
        </p>
      </div>
      {#if approving}
        <div class="approval" data-testid="booster-approval">
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

  <h3 class="type-hub-title">{t("hub.games.booster.times.title")}</h3>
  <p class="intro type-hub-body">{t("hub.games.booster.times.intro")}</p>
  {#if timesFailed}
    <p class="type-hub-body" role="alert">{t("hub.games.booster.times.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void loadTimes()} />
  {:else if devices.length === 0}
    <p class="type-hub-body" data-testid="launch-times-empty">
      {t("hub.games.booster.times.empty")}
    </p>
  {:else}
    <ul class="rows" data-testid="launch-times">
      {#each devices as d (d.device_id)}
        <li class="row" data-testid="launch-times-pc" data-device-id={d.device_id}>
          <span class="type-hub-title" data-testid="launch-times-name">{d.name}</span>
          <ul class="games">
            {#each d.games as g (g.game_id)}
              <li class="type-hub-label tabular-nums" data-testid="launch-times-game">
                {#if g.median_ms === null}
                  {t("hub.games.booster.times.game_none", { game: g.name, count: g.launches })}
                {:else}
                  {t("hub.games.booster.times.game", {
                    game: g.name,
                    seconds: seconds(g.median_ms),
                    count: g.launches,
                  })}
                {/if}
              </li>
            {/each}
          </ul>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .booster,
  .form {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }

  h2,
  h3,
  p,
  ul {
    margin: 0;
  }

  .intro,
  .hint {
    color: var(--text-muted);
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
    box-sizing: border-box;
    padding: var(--space-2) var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
    font-size: var(--type-web-body-size);
  }

  .area {
    width: 100%;
    max-width: 24rem;
    min-height: calc(var(--space-touch-min) * 2);
    resize: vertical;
  }

  .control:focus-visible {
    border-width: 2px;
    border-color: var(--accent);
  }

  .control.invalid {
    border-width: 2px;
    border-color: var(--danger);
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

  .rows {
    display: grid;
    gap: var(--space-2);
    padding: 0;
    list-style: none;
  }

  .row {
    display: grid;
    gap: var(--space-1);
    padding: var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
  }

  .approval {
    display: grid;
    gap: var(--space-2);
  }

  .games {
    display: grid;
    gap: var(--space-1);
    padding: 0;
    list-style: none;
  }
</style>
