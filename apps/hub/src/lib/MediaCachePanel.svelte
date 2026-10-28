<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { MediaCacheStatus } from "@arena/ui/generated";
  import { onMount, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import {
    SESSION_DEPENDENCY,
    loadMediaCache,
    loadSettings,
    newIdempotencyKey,
    submitIntent,
  } from "./auth";
  import { branchLocale } from "./locale.svelte";
  import { KEY_MEDIA_CAP_GB, formatBytes, parseCapGb } from "./media-cache";

  // Settings → System → "Game media cache": how much the Hub's media cache holds against its
  // cap, and the cap itself (a Manager edits it; the Hub applies it at its next pull).
  let { canEdit }: { canEdit: boolean } = $props();

  let cache: MediaCacheStatus | null = $state(null);
  let server: { value: string; rev: number } | null = $state(null);
  let cap = $state("");
  let unavailable = $state(false);
  let loading = $state(true);
  let saving = $state(false);
  let error: MessageKey | null = $state(null);
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let pendingKey: { value: string; key: string } | null = null;
  let loadSeq = 0;

  const changed = $derived.by(() => server !== null && cap.trim() !== server.value);

  async function load(keepText = false) {
    const seq = ++loadSeq;
    loading = true;
    try {
      const [usage, settings] = await Promise.all([loadMediaCache(), loadSettings()]);
      if (seq !== loadSeq) return;
      if (usage.kind === "signed_out" || settings.kind === "signed_out") {
        return void (await invalidate(SESSION_DEPENDENCY));
      }
      cache = usage.kind === "ok" ? usage.value : null;
      unavailable = usage.kind !== "ok";
      const entry =
        settings.kind === "ok" ? settings.settings.find((s) => s.key === KEY_MEDIA_CAP_GB) : null;
      server = entry ? { value: entry.value, rev: entry.rev } : null;
      if (entry && !keepText) cap = entry.value;
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  async function save() {
    if (!canEdit || saving || loading || !changed || !server) return;
    const gb = parseCapGb(cap);
    if (gb === null) {
      error = "hub.settings.system.media.cap_invalid";
      return;
    }
    error = null;
    status = null;
    const value = String(gb);
    if (pendingKey?.value !== value) pendingKey = { value, key: newIdempotencyKey() };
    saving = true;
    try {
      const outcome = await submitIntent(
        { kind: "settings.set", key: KEY_MEDIA_CAP_GB, value },
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
        error = "hub.settings.system.media.cap_invalid";
      } else if (e?.code === "STALE") {
        pendingKey = null;
        status = { tone: "refusal", message: "hub.settings.branch.stale" };
        await load(true);
      } else if (e?.code === "PERMISSION_DENIED" || e?.code === "PIN_REQUIRED") {
        pendingKey = null;
        status = { tone: "refusal", message: "hub.settings.branch.manager_only" };
      } else {
        status = { tone: "refusal", message: "hub.settings.failed" };
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
      if (!saving && branchLocale.changedKeys.includes(KEY_MEDIA_CAP_GB)) void load(changed);
    });
  });

  onMount(() => void load());
</script>

<section class="media" aria-labelledby="settings-media-title" data-testid="settings-media-cache">
  <h3 class="type-hub-title" id="settings-media-title">{t("hub.settings.system.media.title")}</h3>
  <p class="intro type-hub-body">{t("hub.settings.system.media.intro")}</p>

  {#if cache}
    <p class="used type-hub-body" data-testid="media-cache-used">
      {t("hub.settings.system.media.used", {
        used: formatBytes(cache.used_bytes),
        cap: formatBytes(cache.cap_bytes),
      })}
    </p>
    <p class="detail type-hub-label" data-testid="media-cache-detail">
      {t("hub.settings.system.media.detail", {
        covers: formatBytes(cache.cover_bytes),
        videos: formatBytes(cache.video_bytes),
        games: cache.games,
      })}
    </p>
  {:else if unavailable && !loading}
    <p class="status refusal type-hub-label" role="status" data-testid="media-cache-unavailable">
      <span aria-hidden="true">⚠</span>
      <span>{t("hub.settings.system.media.unavailable")}</span>
    </p>
  {/if}

  {#if server}
    <form
      class="form"
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div class="field">
        <label class="label type-hub-label" for="system-media-cap"
          >{t("hub.settings.system.media.cap")}</label
        >
        <div class="row">
          <input
            id="system-media-cap"
            class="control ui-interactive"
            class:invalid={!!error}
            type="text"
            inputmode="numeric"
            autocomplete="off"
            bind:value={cap}
            disabled={!canEdit || saving}
            aria-invalid={error ? "true" : undefined}
            aria-describedby="system-media-cap-hint system-media-cap-error"
          />
          {#if canEdit}
            <Button
              label="hub.settings.system.media.apply"
              type="submit"
              disabled={saving || loading || !changed}
              disabledReason={saving
                ? "hub.settings.saving"
                : loading
                  ? "hub.settings.loading"
                  : "hub.settings.branch.no_changes"}
            />
          {/if}
        </div>
        <p class="hint type-hub-label" id="system-media-cap-hint">
          {t("hub.settings.system.media.cap_hint")}
        </p>
        <p
          class="error type-hub-label"
          id="system-media-cap-error"
          aria-live="polite"
          data-testid="media-cache-error"
        >
          {#if error}<span aria-hidden="true">⚠</span>
            <span>{t(error)}</span>{/if}
        </p>
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="media-cache-status"
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
  .media {
    display: grid;
    gap: var(--space-3);
    max-width: 40rem;
    padding-top: var(--space-4);
    border-top: 1px solid var(--border-strong);
  }

  h3,
  .intro,
  .used,
  .detail {
    margin: 0;
  }

  .intro,
  .detail {
    color: var(--text-muted);
  }

  .used {
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .form,
  .field {
    display: grid;
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
    flex: 0 1 12rem;
    min-width: 8rem;
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
  .error,
  .status {
    margin: 0;
  }

  .hint {
    color: var(--text-muted);
  }

  .error,
  .status {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
  }

  .error {
    color: var(--text-field-error-text);
  }

  .status {
    color: var(--text);
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }
</style>
