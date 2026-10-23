<script lang="ts">
  import { Button, TextField, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { invalidate } from "$app/navigation";
  import {
    SESSION_DEPENDENCY,
    loadSettings,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";

  const KEY = "receipt.footer";

  let footer = $state("");
  /** The setting's `rev` as the Hub last reported it: the edit's `base_rev`. */
  let rev: number | null = $state(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  /** The outcome of the last save, shown next to the control. */
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  /** After a STALE save: the value now on the Hub (the typed text is kept). */
  let serverValue: string | null = $state(null);
  /** The pending save's idempotency key: kept while the same edit is retried. */
  let pendingKey: { value: string; key: string } | null = null;

  async function signedOut() {
    await invalidate(SESSION_DEPENDENCY);
  }

  /** Reads the setting. `keepText` leaves what the user typed in the field. */
  async function load(keepText = false): Promise<boolean> {
    loading = true;
    try {
      const result = await loadSettings();
      if (result.kind === "signed_out") {
        await signedOut();
        return false;
      }
      const entry = result.kind === "ok" ? result.settings.find((s) => s.key === KEY) : undefined;
      if (!entry) {
        loadFailed = true;
        rev = null;
        return false;
      }
      loadFailed = false;
      rev = entry.rev;
      if (keepText) serverValue = entry.value;
      else footer = entry.value;
      return true;
    } finally {
      loading = false;
    }
  }

  async function save() {
    if (saving || rev === null) return;
    saving = true;
    status = null;
    // One key per edit: a retry of the same text reuses it.
    if (pendingKey?.value !== footer) pendingKey = { value: footer, key: newIdempotencyKey() };
    try {
      const outcome = await submitIntent(
        { kind: "settings.set", key: KEY, value: footer },
        pendingKey.key,
        rev,
      );
      if (outcome.kind === "signed_out") return signedOut();
      if (outcome.kind === "applied") {
        pendingKey = null;
        serverValue = null;
        status = { tone: "ok", message: "hub.settings.saved" };
        // The Hub's rev, not a guess.
        await load();
      } else if (outcome.kind === "error" && outcome.error.code === "PERMISSION_DENIED") {
        // Refused inline, next to the control; never a modal and never naming anyone.
        pendingKey = null;
        status = { tone: "refusal", message: "hub.refusal.permission" };
      } else if (outcome.kind === "error" && outcome.error.code === "STALE") {
        pendingKey = null;
        status = { tone: "refusal", message: "hub.settings.stale" };
        await load(true);
      } else {
        status = { tone: "refusal", message: "hub.settings.failed" };
      }
    } finally {
      saving = false;
    }
  }

  onMount(() => {
    void load();
  });
</script>

<section class="receipt" aria-labelledby="settings-receipt-title">
  <h2 class="type-hub-title" id="settings-receipt-title">{t("hub.settings.nav.receipt")}</h2>
  <form
    class="demo"
    data-testid="settings-receipt-footer"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <TextField label="hub.settings.receipt_footer" name="receipt_footer" bind:value={footer} />
    {#if loadFailed}
      <div class="actions" data-testid="settings-load-failed">
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
      <div class="actions">
        <Button
          label="hub.settings.save"
          type="submit"
          disabled={saving || rev === null}
          disabledReason={saving ? "hub.settings.saving" : "hub.settings.loading"}
        />
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="settings-status"
        >
          {#if status}
            {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{t(status.message)}</span>
          {/if}
        </p>
      </div>
      {#if serverValue !== null}
        <p class="type-hub-label server" data-testid="settings-server-value">
          {t("hub.settings.server_value", { value: serverValue })}
        </p>
      {/if}
    {/if}
  </form>
</section>

<style>
  .receipt h2 {
    margin: 0;
  }

  .demo {
    display: grid;
    gap: var(--space-4);
    max-width: 32rem;
    margin-top: var(--space-4);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }

  .status,
  .server {
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
