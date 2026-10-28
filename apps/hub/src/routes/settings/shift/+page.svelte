<script lang="ts">
  import { Button, InlinePin, TextField, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, untrack } from "svelte";
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
  import { sessionStaff } from "../../../lib/operating";
  import { KEY_VARIANCE_THRESHOLD, floatText, parseFloat } from "../../../lib/shift";

  const exponent = $derived(branchLocale.current?.exponent ?? 2);

  let server = $state<{ minor: number; rev: number } | null>(null);
  let text = $state("");
  let error = $state(null as MessageKey | null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; text: string } | null = $state(null);
  let saveWrap: HTMLElement | undefined = $state();
  let idem: { value: string; key: string } | null = null;

  const staff = $derived(sessionStaff(page.data));
  const needsApproval = $derived(!hasPermission(staff, "settings.edit"));
  const changed = $derived(server !== null && text.trim() !== floatText(server.minor, exponent));
  const saveButton = $derived(saveWrap?.querySelector("button") ?? null);

  async function load(keepEdit = false) {
    loading = true;
    try {
      const result = await loadSettings();
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const entry =
        result.kind === "ok" ? result.settings.find((s) => s.key === KEY_VARIANCE_THRESHOLD) : null;
      const minor = entry ? Number(entry.value) : Number.NaN;
      if (!entry || !Number.isSafeInteger(minor) || minor < 0) {
        loadFailed = true;
        return;
      }
      loadFailed = false;
      server = { minor, rev: entry.rev };
      if (!keepEdit) text = floatText(minor, exponent);
    } finally {
      loading = false;
    }
  }

  async function approve(pin: string) {
    const outcome = await elevate(
      [{ intent_kind: "settings.set", key: KEY_VARIANCE_THRESHOLD }],
      pin,
    );
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    if (!approving) {
      void dropElevation(outcome.ticket);
      return { ok: true } as const;
    }
    approving = false;
    void save(outcome.ticket);
    return { ok: true } as const;
  }

  async function save(ticket?: string) {
    if (!server || saving || loading) {
      if (ticket) void dropElevation(ticket);
      return;
    }
    const minor = parseFloat(text, exponent);
    if (minor === null) {
      error = "hub.settings.shift.err.threshold";
      if (ticket) void dropElevation(ticket);
      return;
    }
    error = null;
    if (!changed) {
      if (ticket) void dropElevation(ticket);
      return;
    }
    if (needsApproval && !ticket) {
      approvalPrompt = "ui.pin.prompt";
      approving = true;
      return;
    }
    saving = true;
    status = null;
    let again = false;
    let failed = false;
    try {
      const value = String(minor);
      if (idem?.value !== value) idem = { value, key: newIdempotencyKey() };
      const outcome = await submitIntent(
        { kind: "settings.set", key: KEY_VARIANCE_THRESHOLD, value },
        idem.key,
        server.rev,
        { elevationTicket: ticket },
      );
      if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (outcome.kind === "applied") {
        idem = null;
        status = { tone: "ok", text: t("hub.settings.saved") };
      } else {
        failed = true;
        const code = outcome.kind === "error" ? outcome.error.code : null;
        if (code === "PIN_REQUIRED") again = true;
        else if (code === "STALE")
          status = { tone: "refusal", text: t("hub.settings.branch.stale") };
        else if (code === "PERMISSION_DENIED")
          status = { tone: "refusal", text: t("hub.settings.branch.manager_only") };
        else status = { tone: "refusal", text: t("hub.settings.failed") };
      }
      await load(failed && !status);
    } finally {
      saving = false;
    }
    if (again) {
      approvalPrompt = "ui.pin.again";
      approving = true;
    }
  }

  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      if (!saving && branchLocale.changedKeys.includes(KEY_VARIANCE_THRESHOLD)) void load();
    });
  });

  onMount(() => {
    void load();
  });
</script>

<section class="shift" aria-labelledby="settings-shift-title">
  <h2 class="type-hub-title" id="settings-shift-title">{t("hub.settings.shift.title")}</h2>
  {#if loadFailed}
    <div class="actions" data-testid="shift-settings-load-failed">
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
    <p class="type-hub-body muted">{t("hub.settings.shift.intro")}</p>
    <form
      class="form"
      data-testid="settings-shift"
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <TextField
        label="hub.settings.shift.threshold"
        bind:value={text}
        inputmode="decimal"
        autocomplete="off"
        {error}
      />
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
                : "hub.settings.branch.no_changes"}
          />
        </span>
        {#if needsApproval && !approving}
          <p class="type-hub-label" data-testid="shift-settings-needs-approval">
            {t("hub.settings.branch.needs_approval")}
          </p>
        {/if}
        <p
          class="status type-hub-label"
          class:refusal={status?.tone === "refusal"}
          role="status"
          data-testid="shift-settings-status"
        >
          {#if status}
            {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{status.text}</span>
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
  .shift {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  p {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .form {
    display: grid;
    gap: var(--space-4);
    max-width: 40rem;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .status {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
    color: var(--text);
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }
</style>
