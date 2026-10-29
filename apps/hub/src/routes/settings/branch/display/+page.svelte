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
  import { KEY_DISPLAY_MODES, sessionStaff } from "../../../../lib/operating";

  // Settings → Branch → Kiosk display (Story 6.35): the display modes a player may pick in the
  // kiosk's PC settings. The Hub checks the list (`WxH@Hz`, each once); an empty list means each
  // PC keeps its own current mode. Connected kiosks get a change at once.

  type Stored = { value: string; rev: number };
  let server = $state<Stored | null>(null);
  let modes = $state("");
  let error: MessageKey | null = $state(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let pending: { value: string; key: string } | null = null;
  let loadSeq = 0;

  const staff = $derived(sessionStaff(page.data));
  const canEdit = $derived(hasPermission(staff, "settings.edit"));
  const describedBy = $derived(
    ["display-modes-error", "display-modes-help"]
      .filter((id) => id !== "display-modes-error" || error)
      .join(" "),
  );
  const changed = $derived(server !== null && modes.trim() !== server.value.trim());

  async function load(keepEdit = false) {
    const seq = ++loadSeq;
    loading = true;
    try {
      const result = await loadSettings();
      if (seq !== loadSeq) return;
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const entry =
        result.kind === "ok" ? result.settings.find((s) => s.key === KEY_DISPLAY_MODES) : null;
      if (!entry) {
        loadFailed = true;
        return;
      }
      loadFailed = false;
      server = { value: entry.value, rev: entry.rev };
      if (!keepEdit) modes = entry.value;
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  async function save() {
    if (!canEdit || saving || loading || !changed || !server) return;
    saving = true;
    status = null;
    error = null;
    try {
      const value = modes.trim();
      if (pending?.value !== value) pending = { value, key: newIdempotencyKey() };
      const outcome = await submitIntent(
        { kind: "settings.set", key: KEY_DISPLAY_MODES, value },
        pending.key,
        server.rev,
      );
      if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (outcome.kind === "applied") {
        pending = null;
        await load();
        status = { tone: "ok", message: "hub.settings.saved" };
        return;
      }
      const code = outcome.kind === "error" ? outcome.error.code : null;
      if (code === "VALIDATION_FAILED") {
        pending = null;
        error = "hub.settings.kiosk_display.invalid";
      } else if (code === "STALE") {
        pending = null;
        error = "hub.settings.branch.stale";
        status = { tone: "refusal", message: "hub.settings.branch.stale" };
      } else if (code === "PERMISSION_DENIED") {
        pending = null;
        status = { tone: "refusal", message: "hub.settings.branch.manager_only" };
      } else {
        status = { tone: "refusal", message: "hub.settings.failed" };
      }
      await load(true);
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
      if (!saving && branchLocale.changedKeys.includes(KEY_DISPLAY_MODES)) void load(changed);
    });
  });

  onMount(() => {
    void load();
  });
</script>

<section class="display" aria-labelledby="settings-display-title">
  <h2 class="type-hub-title" id="settings-display-title">
    {t("hub.settings.kiosk_display.title")}
  </h2>
  <p class="intro type-hub-body">{t("hub.settings.kiosk_display.intro")}</p>

  {#if loadFailed}
    <div class="actions" data-testid="display-load-failed">
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
      data-testid="settings-display"
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div class="field">
        <label class="label type-hub-label" for="display-modes"
          >{t("hub.settings.kiosk_display.modes")}</label
        >
        <textarea
          id="display-modes"
          class="control ui-interactive"
          class:invalid={!!error}
          rows="3"
          autocomplete="off"
          spellcheck="false"
          bind:value={modes}
          disabled={!canEdit || saving}
          aria-invalid={error ? "true" : undefined}
          aria-describedby={describedBy}></textarea>
        <p class="hint type-hub-label" id="display-modes-help">
          {t("hub.settings.kiosk_display.modes_help")}
        </p>
        <p class="error type-hub-label" id="display-modes-error" aria-live="polite">
          {#if error}<span aria-hidden="true">⚠</span> <span>{t(error)}</span>{/if}
        </p>
      </div>

      <div class="actions">
        <Button
          label="hub.settings.save"
          type="submit"
          disabled={!canEdit || saving || loading || !changed}
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
          data-testid="display-status"
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
  .display {
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
  }

  .label {
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .control {
    --ui-bg: var(--text-field-background);
    box-sizing: border-box;
    width: 100%;
    min-height: calc(var(--space-touch-min) * 2);
    padding: var(--space-2) var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
    font-size: var(--type-web-body-size);
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

  .control:disabled {
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
    min-height: 1lh;
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
