<script lang="ts">
  import { BrandMark, Button, t } from "@arena/ui";
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
  import { KEY_KIOSK_LOGO, prepareLogo } from "../../../../lib/logo";
  import { sessionStaff } from "../../../../lib/operating";

  let server = $state<{ value: string; rev: number } | null>(null);
  /** A chosen logo waiting to be saved (a data URI). */
  let draft: string | null = $state(null);
  let error: MessageKey | null = $state(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let input: HTMLInputElement | undefined = $state();
  let pending: { value: string; key: string } | null = null;
  let loadSeq = 0;

  const staff = $derived(sessionStaff(page.data));
  const canEdit = $derived(hasPermission(staff, "settings.edit"));
  const changed = $derived(server !== null && draft !== null && draft !== server.value);
  const shown = $derived(draft ?? server?.value ?? "");

  async function load(keepDraft = false) {
    const seq = ++loadSeq;
    loading = true;
    try {
      const result = await loadSettings();
      if (seq !== loadSeq) return;
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const entry =
        result.kind === "ok" ? result.settings.find((s) => s.key === KEY_KIOSK_LOGO) : undefined;
      if (!entry) {
        loadFailed = true;
        return;
      }
      loadFailed = false;
      server = { value: entry.value, rev: entry.rev };
      if (!keepDraft) draft = null;
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  /** Counts the picks, so a slow resize cannot overwrite a later choice. */
  let picks = 0;

  async function choose(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    status = null;
    error = null;
    if (!file) return;
    const mine = ++picks;
    const outcome = await prepareLogo(file);
    if (input) input.value = "";
    if (mine !== picks) return; // a later pick supersedes this one
    if (outcome.kind === "ok") {
      draft = outcome.uri;
      return;
    }
    draft = null;
    error = (
      {
        type: "hub.settings.logo.invalid.type",
        read: "hub.settings.logo.invalid.read",
        size: "hub.settings.logo.invalid.size",
        svg: "hub.settings.logo.invalid.svg",
      } as const
    )[outcome.kind];
  }

  /** Stores `value` as the logo ("" removes it). */
  async function commit(value: string) {
    if (!canEdit || saving || loading || server === null) return;
    saving = true;
    status = null;
    error = null;
    try {
      if (pending?.value !== value) pending = { value, key: newIdempotencyKey() };
      const outcome = await submitIntent(
        { kind: "settings.set", key: KEY_KIOSK_LOGO, value },
        pending.key,
        server.rev,
      );
      if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (outcome.kind === "applied") {
        pending = null;
        draft = null;
        await load();
        status = {
          tone: "ok",
          message: value === "" ? "hub.settings.logo.removed" : "hub.settings.saved",
        };
        return;
      }
      const code = outcome.kind === "error" ? outcome.error.code : null;
      if (code === "VALIDATION_FAILED") {
        pending = null;
        error = "hub.settings.logo.invalid.server";
      } else if (code === "STALE") {
        pending = null;
        error = "hub.settings.branch.stale";
        status = { tone: "refusal", message: "hub.settings.branch.stale" };
        await load(true);
      } else if (code === "PERMISSION_DENIED") {
        pending = null;
        status = { tone: "refusal", message: "hub.settings.branch.manager_only" };
      } else {
        status = {
          tone: "refusal",
          message: value === "" ? "hub.settings.logo.remove_failed" : "hub.settings.failed",
        };
      }
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
      if (!saving && branchLocale.changedKeys.includes(KEY_KIOSK_LOGO)) void load(draft !== null);
    });
  });

  onMount(() => {
    void load();
  });
</script>

<section class="logo" aria-labelledby="settings-logo-title">
  <h2 class="type-hub-title" id="settings-logo-title">{t("hub.settings.logo.title")}</h2>
  <p class="intro type-hub-body">{t("hub.settings.logo.intro")}</p>

  {#if loadFailed}
    <div class="actions" data-testid="logo-load-failed">
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
      data-testid="settings-logo"
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        if (draft !== null) void commit(draft);
      }}
    >
      <div class="field">
        <p class="label type-hub-label">
          {draft !== null ? t("hub.settings.logo.preview") : t("hub.settings.logo.current")}
        </p>
        <div class="frame" data-testid="logo-frame">
          {#if shown !== ""}
            <img
              class="preview"
              data-testid="logo-preview"
              src={shown}
              alt={t("hub.settings.logo.alt")}
            />
          {:else}
            <BrandMark size={64} data-testid="logo-default" />
            <p class="none type-hub-body" data-testid="logo-none">{t("hub.settings.logo.none")}</p>
          {/if}
        </div>
      </div>

      <div class="field">
        <label class="label type-hub-label" for="logo-file">{t("hub.settings.logo.choose")}</label>
        <input
          id="logo-file"
          class="file ui-interactive"
          data-testid="logo-file"
          type="file"
          accept=".png,.svg,image/png,image/svg+xml"
          bind:this={input}
          onchange={choose}
          disabled={!canEdit || saving}
          aria-describedby={error ? "logo-error" : undefined}
        />
        <p class="error type-hub-label" id="logo-error" aria-live="polite" data-testid="logo-error">
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
        <Button
          label="hub.settings.logo.remove"
          variant="secondary"
          disabled={!canEdit || saving || loading || server === null || server.value === ""}
          disabledReason={!canEdit
            ? "hub.settings.branch.manager_only"
            : saving
              ? "hub.settings.saving"
              : loading
                ? "hub.settings.loading"
                : "hub.settings.logo.none"}
          onclick={() => void commit("")}
        />
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="logo-status"
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
  .logo {
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
    margin: 0;
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .frame {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    min-height: 96px;
    padding: var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--bg);
  }

  .preview {
    display: block;
    max-width: 100%;
    max-height: 128px;
    object-fit: contain;
  }

  .none {
    margin: 0;
    color: var(--text-muted);
  }

  .file {
    --ui-bg: transparent;
    min-height: var(--space-touch-min);
    max-width: 100%;
    color: var(--text);
  }

  .file:disabled {
    opacity: 0.4;
    cursor: not-allowed;
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
