<script lang="ts">
  import { Button, t } from "@arena/ui";
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
  import { KEY_BACKUP_DIR, sessionStaff } from "../../../lib/operating";

  type DialogOpen = (options: {
    directory: boolean;
    multiple: boolean;
  }) => Promise<string | string[] | null>;

  const REASONS: Record<string, MessageKey> = {
    not_found: "hub.settings.system.invalid.not_found",
    not_dir: "hub.settings.system.invalid.not_dir",
    not_writable: "hub.settings.system.invalid.not_writable",
  };

  let server: { value: string; rev: number } | null = $state(null);
  let path = $state("");
  let error: MessageKey | null = $state(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let input: HTMLInputElement | undefined = $state();
  let pendingKey: { value: string; key: string } | null = null;
  let loadSeq = 0;
  let dialogOpen: DialogOpen | null = $state(null);

  const staff = $derived(sessionStaff(page.data));
  const canEdit = $derived(hasPermission(staff, "settings.edit"));
  const changed = $derived.by(() => server !== null && path.trim() !== server.value);

  async function load(keepText = false) {
    const seq = ++loadSeq;
    loading = true;
    try {
      const result = await loadSettings();
      if (seq !== loadSeq) return;
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const entry =
        result.kind === "ok" ? result.settings.find((s) => s.key === KEY_BACKUP_DIR) : undefined;
      if (!entry) {
        loadFailed = true;
        return;
      }
      loadFailed = false;
      server = { value: entry.value, rev: entry.rev };
      if (!keepText) path = entry.value;
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  async function browse() {
    if (!dialogOpen || !canEdit || saving) return;
    try {
      const picked = await dialogOpen({ directory: true, multiple: false });
      const chosen = Array.isArray(picked) ? picked[0] : picked;
      if (typeof chosen === "string" && chosen) {
        path = chosen;
        error = null;
      }
    } catch {
      status = { tone: "refusal", message: "hub.settings.failed" };
    }
  }

  async function save() {
    if (!canEdit || saving || loading || !changed || !server) return;
    const value = path.trim();
    error = null;
    status = null;
    if (pendingKey?.value !== value) pendingKey = { value, key: newIdempotencyKey() };
    saving = true;
    try {
      const outcome = await submitIntent(
        { kind: "settings.set", key: KEY_BACKUP_DIR, value },
        pendingKey.key,
        server.rev,
      );
      if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (outcome.kind === "applied") {
        pendingKey = null;
        status = { tone: "ok", message: "hub.settings.saved" };
        await load();
        return;
      }
      const e = outcome.kind === "error" ? outcome.error : null;
      if (e?.code === "VALIDATION_FAILED") {
        pendingKey = null;
        error = REASONS[e.message] ?? "hub.settings.system.invalid.path";
      } else if (e?.code === "STALE") {
        pendingKey = null;
        status = { tone: "refusal", message: "hub.settings.branch.stale" };
        await load(true);
      } else if (e?.code === "PERMISSION_DENIED") {
        pendingKey = null;
        status = { tone: "refusal", message: "hub.settings.branch.manager_only" };
      } else {
        status = { tone: "refusal", message: "hub.settings.failed" };
      }
    } finally {
      saving = false;
    }
    if (error) {
      await tick();
      input?.focus();
    }
  }

  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      if (!saving && branchLocale.changedKeys.includes(KEY_BACKUP_DIR)) void load(changed);
    });
  });

  onMount(() => {
    const tauri = (window as { __TAURI__?: { dialog?: { open?: DialogOpen } } }).__TAURI__;
    const open = tauri?.dialog?.open;
    if (typeof open === "function") dialogOpen = open;
    void load();
  });

  const describedBy = $derived(
    ["system-backup-hint", error ? "system-backup-error" : null].filter(Boolean).join(" "),
  );
</script>

<section class="system" aria-labelledby="settings-system-title">
  <h2 class="type-hub-title" id="settings-system-title">{t("hub.settings.system.title")}</h2>
  <p class="intro type-hub-body">{t("hub.settings.system.intro")}</p>

  {#if loadFailed}
    <div class="actions" data-testid="system-load-failed">
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
      data-testid="settings-system"
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div class="field">
        <label class="label type-hub-label" for="system-backup-dir"
          >{t("hub.settings.system.backup_dir")}</label
        >
        <div class="row">
          <input
            id="system-backup-dir"
            class="control ui-interactive"
            class:invalid={!!error}
            type="text"
            autocomplete="off"
            spellcheck="false"
            bind:this={input}
            bind:value={path}
            disabled={!canEdit || saving}
            aria-invalid={error ? "true" : undefined}
            aria-describedby={describedBy}
          />
          {#if dialogOpen}
            <Button
              label="hub.settings.system.browse"
              variant="secondary"
              disabled={!canEdit || saving}
              disabledReason={!canEdit ? "hub.settings.branch.manager_only" : "hub.settings.saving"}
              onclick={() => void browse()}
            />
          {/if}
        </div>
        <p class="hint type-hub-label" id="system-backup-hint">
          {t("hub.settings.system.backup_hint")}
        </p>
        <p
          class="error type-hub-label"
          id="system-backup-error"
          aria-live="polite"
          data-testid="system-backup-error"
        >
          {#if error}<span aria-hidden="true">⚠</span>
            <span>{t(error)}</span>{/if}
        </p>
        {#if server && server.value === "" && !loading}
          <p class="note type-hub-label" data-testid="system-backup-unset">
            {t("hub.settings.system.not_set")}
          </p>
        {/if}
      </div>

      <div class="actions">
        <Button
          label="hub.settings.save"
          type="submit"
          disabled={!canEdit || saving || loading || !changed}
          disabledReason={!canEdit
            ? "hub.settings.branch.manager_only"
            : saving
              ? "hub.settings.system.checking"
              : loading
                ? "hub.settings.loading"
                : "hub.settings.branch.no_changes"}
        />
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="system-status"
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
  .system {
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
    max-width: 40rem;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .label {
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .control {
    --ui-bg: var(--text-field-background);
    flex: 1;
    min-width: 12rem;
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

  .control:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .hint,
  .note,
  .error {
    margin: 0;
  }

  .hint {
    color: var(--text-muted);
  }

  .note {
    color: var(--text);
  }

  .error {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
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
