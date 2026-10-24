<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, untrack } from "svelte";
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
  import { KEY_FULFILMENT, KEY_KIOSK_VOLUME, sessionStaff } from "../../../../lib/operating";

  type Field = "fulfilment" | "volume";
  const FIELDS: readonly Field[] = ["fulfilment", "volume"];
  const KEYS: Record<Field, string> = { fulfilment: KEY_FULFILMENT, volume: KEY_KIOSK_VOLUME };
  const INVALID: Record<Field, MessageKey> = {
    fulfilment: "hub.settings.service.invalid.fulfilment",
    volume: "hub.settings.service.invalid.volume",
  };
  const MODES: readonly (readonly [string, MessageKey])[] = [
    ["seat", "hub.settings.service.fulfilment_seat"],
    ["counter", "hub.settings.service.fulfilment_counter"],
  ];

  let server: Partial<Record<Field, { value: string; rev: number }>> = $state({});
  let form: Record<Field, string> = $state({ fulfilment: "", volume: "50" });
  let errors: Partial<Record<Field, MessageKey>> = $state({});
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  const pendingKeys = new Map<Field, { value: string; key: string }>();
  let loadSeq = 0;

  const staff = $derived(sessionStaff(page.data));
  const canEdit = $derived(hasPermission(staff, "settings.edit"));
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

  async function save() {
    if (!canEdit || saving || loading || changed.length === 0) return;
    saving = true;
    status = null;
    errors = {};
    const edits = [...changed];
    let failedAt: Field | null = null;
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
          errors = { [f]: "hub.settings.branch.stale" };
          status = { tone: "refusal", message: "hub.settings.branch.stale" };
        } else if (code === "PERMISSION_DENIED") {
          pendingKeys.delete(f);
          status = { tone: "refusal", message: "hub.settings.branch.manager_only" };
        } else {
          status = { tone: "refusal", message: "hub.settings.failed" };
        }
        break;
      }
      await load(failedAt === null ? [] : edits.slice(edits.indexOf(failedAt)));
      if (failedAt === null) status = { tone: "ok", message: "hub.settings.saved" };
    } finally {
      saving = false;
    }
  }

  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      const keys = branchLocale.changedKeys;
      if (!saving && FIELDS.some((f) => keys.includes(KEYS[f]))) void load([...changed]);
    });
  });

  onMount(() => {
    void load();
  });

  const describedBy = (f: Field, extra?: string) =>
    [errors[f] ? "service-" + f + "-error" : null, extra].filter(Boolean).join(" ") || undefined;
</script>

{#snippet fieldError(f: Field)}
  <p class="error type-hub-label" id={"service-" + f + "-error"} aria-live="polite">
    {#if errors[f]}<span aria-hidden="true">⚠</span>
      <span>{t(errors[f]!)}</span>{/if}
  </p>
{/snippet}

<section class="service" aria-labelledby="settings-service-title">
  <h2 class="type-hub-title" id="settings-service-title">{t("hub.settings.service.title")}</h2>
  <p class="intro type-hub-body">{t("hub.settings.service.intro")}</p>

  {#if loadFailed}
    <div class="actions" data-testid="service-load-failed">
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
      data-testid="settings-service"
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <fieldset class="field group" aria-describedby={describedBy("fulfilment")}>
        <legend class="label type-hub-label">{t("hub.settings.service.fulfilment")}</legend>
        <div class="options">
          {#each MODES as [value, label] (value)}
            <label class="option type-hub-body">
              <input
                type="radio"
                name="fulfilment"
                {value}
                bind:group={form.fulfilment}
                disabled={!canEdit || saving}
              />
              <span>{t(label)}</span>
            </label>
          {/each}
        </div>
        {@render fieldError("fulfilment")}
      </fieldset>

      <div class="field">
        <label class="label type-hub-label" for="service-volume"
          >{t("hub.settings.service.volume")}</label
        >
        <div class="slider">
          <input
            id="service-volume"
            class="range ui-interactive"
            type="range"
            min="0"
            max="100"
            step="1"
            bind:value={() => Number(form.volume), (v) => (form.volume = String(v))}
            disabled={!canEdit || saving}
            aria-describedby={describedBy("volume", "service-volume-range")}
          />
          <output
            class="value tabular-nums type-hub-title"
            for="service-volume"
            data-testid="service-volume-value">{form.volume}</output
          >
        </div>
        <p class="hint type-hub-label" id="service-volume-range">
          {t("hub.settings.service.volume_range")}
        </p>
        {@render fieldError("volume")}
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
          data-testid="service-status"
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
  .service {
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

  .slider {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .range {
    --ui-bg: transparent;
    flex: 1;
    min-width: 0;
    height: var(--space-touch-min);
    margin: 0;
    accent-color: var(--accent);
  }

  .value {
    min-width: 3ch;
    color: var(--text);
    text-align: end;
  }

  .range:disabled,
  .option:has(input:disabled) {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .hint {
    margin: 0;
    color: var(--text-muted);
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
