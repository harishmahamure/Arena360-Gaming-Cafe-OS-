<script lang="ts">
  import { Button, InlinePin, t } from "@arena/ui";
  import type { SessionStaff } from "@arena/ui/generated";
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
  } from "../../../lib/auth";
  import { branchLocale } from "../../../lib/locale.svelte";

  const KEY = "idle_lock_minutes";
  const DESCRIBED = "screen-lock-range screen-lock-next"; // i18n-ignore (element ids)
  const DESCRIBED_INVALID = "screen-lock-error screen-lock-range"; // i18n-ignore (element ids)

  /** Whether `value` is a whole number of minutes from 1 to 30 (as the Hub accepts it). */
  const valid = (value: string) => /^\d{1,2}$/.test(value) && +value >= 1 && +value <= 30;

  let value = $state("");
  /** The setting as the Hub last reported it; `rev` is the edit's `base_rev`. */
  let server = $state<{ value: string; rev: number } | null>(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let invalid = $state(false);
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let pendingKey: { value: string; key: string } | null = null;
  let input: HTMLInputElement | undefined = $state();
  let saveWrap: HTMLElement | undefined = $state();
  const saveButton = $derived(saveWrap?.querySelector("button") ?? null);

  const staff = $derived.by(() => {
    const session = (page.data as { session?: { state: string; staff?: SessionStaff } }).session;
    return session?.state === "signed_in" ? (session.staff ?? null) : null;
  });
  const needsApproval = $derived(!hasPermission(staff, "settings.edit"));
  const changed = $derived(server !== null && value !== server.value);

  async function load(keepEdit = false) {
    loading = true;
    try {
      const result = await loadSettings();
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const entry = result.kind === "ok" ? result.settings.find((s) => s.key === KEY) : undefined;
      loadFailed = !entry;
      if (!entry) return;
      server = { value: entry.value, rev: entry.rev };
      if (!keepEdit) value = entry.value;
    } finally {
      loading = false;
    }
  }

  async function focusInvalid() {
    await tick();
    input?.focus();
  }

  async function save(ticket?: string) {
    if (saving || !server || !changed) {
      if (ticket) void dropElevation(ticket);
      return;
    }
    status = null;
    // Checked here first: an out-of-range value never costs a round trip.
    invalid = !valid(value);
    if (invalid) {
      if (ticket) void dropElevation(ticket);
      return focusInvalid();
    }
    if (needsApproval && !ticket) {
      approvalPrompt = "ui.pin.prompt";
      approving = true;
      return;
    }
    if (pendingKey?.value !== value) pendingKey = { value, key: newIdempotencyKey() };
    saving = true;
    let pinAgain = false;
    try {
      const outcome = await submitIntent(
        { kind: "settings.set", key: KEY, value },
        pendingKey.key,
        server.rev,
        { elevationTicket: ticket },
      );
      if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (outcome.kind === "applied") {
        pendingKey = null;
        status = { tone: "ok", message: "hub.settings.saved" };
        await load();
        if (ticket) {
          await tick();
          saveButton?.focus();
        }
        return;
      }
      const code = outcome.kind === "error" ? outcome.error.code : null;
      if (code === "VALIDATION_FAILED") {
        pendingKey = null;
        invalid = true;
        return focusInvalid();
      }
      if (code === "PIN_REQUIRED") pinAgain = true;
      else if (code === "STALE") {
        pendingKey = null;
        status = { tone: "refusal", message: "hub.settings.stale" };
        await load(true);
      } else if (code === "PERMISSION_DENIED") {
        pendingKey = null;
        status = { tone: "refusal", message: "hub.refusal.permission" };
      } else status = { tone: "refusal", message: "hub.settings.failed" };
    } finally {
      saving = false;
    }
    if (pinAgain) {
      approvalPrompt = "ui.pin.again";
      approving = true;
    }
  }

  /** A manager's PIN approves exactly this one setting. */
  async function approve(pin: string) {
    const outcome = await elevate([{ intent_kind: "settings.set", key: KEY }], pin);
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

  // Another window changed settings: refresh, keeping an unsaved edit.
  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      if (!saving) void load(changed);
    });
  });

  onMount(() => {
    void load();
  });
</script>

<section class="screen-lock" aria-labelledby="settings-screen-lock-title">
  <h2 class="type-hub-title" id="settings-screen-lock-title">
    {t("hub.settings.nav.screen_lock")}
  </h2>
  <p class="intro type-hub-body">{t("hub.settings.screen_lock.intro")}</p>
  {#if loadFailed}
    <div class="actions" data-testid="screen-lock-load-failed">
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
      data-testid="settings-screen-lock"
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div class="field">
        <label class="label type-hub-label" for="screen-lock-minutes"
          >{t("hub.settings.screen_lock.minutes")}</label
        >
        <input
          id="screen-lock-minutes"
          bind:this={input}
          class="control ui-interactive tabular-nums"
          class:invalid
          type="number"
          inputmode="numeric"
          min="1"
          max="30"
          step="1"
          {value}
          oninput={(e) => {
            value = e.currentTarget.value;
            invalid = false;
          }}
          disabled={saving || approving}
          aria-invalid={invalid ? "true" : undefined}
          aria-describedby={invalid ? DESCRIBED_INVALID : DESCRIBED}
        />
        <p class="note type-hub-label" id="screen-lock-range" data-testid="screen-lock-range">
          {t("hub.settings.screen_lock.range")}
        </p>
        <p class="error type-hub-label" id="screen-lock-error" aria-live="polite">
          {#if invalid}<span aria-hidden="true">⚠</span>
            <span>{t("hub.settings.screen_lock.invalid")}</span>{/if}
        </p>
        <p class="note type-hub-label" id="screen-lock-next">
          {t("hub.settings.screen_lock.next_period")}
        </p>
      </div>
      <div class="actions">
        <span bind:this={saveWrap}>
          <Button
            label="hub.settings.save"
            type="submit"
            disabled={saving || approving || loading || !changed}
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
          <p class="note type-hub-label" data-testid="screen-lock-needs-approval">
            {t("hub.settings.branch.needs_approval")}
          </p>
        {/if}
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="screen-lock-status"
        >
          {#if status}
            {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{t(status.message)}</span>
          {/if}
        </p>
      </div>
      {#if approving}
        <InlinePin
          message={approvalPrompt}
          onsubmit={approve}
          oncancel={() => (approving = false)}
          returnFocusTo={saveButton}
        />
      {/if}
    </form>
  {/if}
</section>

<style>
  .screen-lock {
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
  }

  .label {
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .control {
    --ui-bg: var(--text-field-background);
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    max-width: 8rem;
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

  .label,
  .note,
  .error,
  .status {
    margin: 0;
  }

  .note {
    color: var(--text);
  }

  .error,
  .status {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
  }

  .error,
  .status.refusal {
    color: var(--text-field-error-text);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
</style>
