<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import {
    SESSION_DEPENDENCY,
    loadSettings,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { branchLocale } from "../../../lib/locale.svelte";
  import {
    KEY_BRANCH_NAME,
    KEY_TIER,
    NOTIFY_KEYS,
    NOTIFY_MESSAGES,
    isPaidTier,
  } from "../../../lib/receipt";

  type Row = { value: string; rev: number };

  const READ = [...NOTIFY_KEYS, KEY_TIER, KEY_BRANCH_NAME];
  const LABELS: Record<(typeof NOTIFY_MESSAGES)[number]["id"], MessageKey> = {
    whatsapp_receipt: "hub.settings.notifications.whatsapp_receipt",
    credit_reminder: "hub.settings.notifications.credit_reminder",
    low_balance: "hub.settings.notifications.low_balance",
    session_ending: "hub.settings.notifications.session_ending",
  };
  const TEMPLATES: Record<(typeof NOTIFY_MESSAGES)[number]["id"], MessageKey> = {
    whatsapp_receipt: "hub.settings.notifications.whatsapp_receipt.template",
    credit_reminder: "hub.settings.notifications.credit_reminder.template",
    low_balance: "hub.settings.notifications.low_balance.template",
    session_ending: "hub.settings.notifications.session_ending.template",
  };

  let rows: Record<string, Row> = $state({});
  let loading = $state(true);
  let loadFailed = $state(false);
  /** The key being saved: its switch is disabled meanwhile. */
  let busy: string | null = $state(null);
  let status: { tone: "ok" | "refusal"; text: string } | null = $state(null);

  const paid = $derived(isPaidTier(rows[KEY_TIER]?.value));
  const branchName = $derived(rows[KEY_BRANCH_NAME]?.value ?? "");

  /** The template with sample values, so staff see how the message reads. */
  const preview = (id: keyof typeof TEMPLATES) =>
    t(TEMPLATES[id], {
      name: t("hub.settings.notifications.sample_name"),
      branch: branchName,
      amount: t("hub.settings.notifications.sample_amount"),
      link: t("hub.settings.notifications.sample_link"),
    });

  async function load(quiet = false): Promise<void> {
    if (!quiet) loading = true;
    try {
      const result = await loadSettings();
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const next: Record<string, Row> = {};
      if (result.kind === "ok") {
        for (const s of result.settings) {
          if (READ.includes(s.key)) next[s.key] = { value: s.value, rev: s.rev };
        }
      }
      loadFailed = NOTIFY_KEYS.some((k) => next[k] === undefined);
      if (!loadFailed) rows = next;
    } finally {
      loading = false;
    }
  }

  async function flip(key: string, box: HTMLInputElement) {
    if (busy || !rows[key]) return;
    busy = key;
    status = null;
    const value = rows[key].value === "true" ? "false" : "true";
    try {
      const outcome = await submitIntent(
        { kind: "settings.set", key, value },
        newIdempotencyKey(),
        rows[key].rev,
      );
      if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (outcome.kind === "applied") status = { tone: "ok", text: t("hub.settings.saved") };
      else {
        const code = outcome.kind === "error" ? outcome.error.code : null;
        const message: MessageKey =
          code === "PERMISSION_DENIED"
            ? "hub.refusal.permission"
            : code === "STALE"
              ? "hub.settings.notifications.stale"
              : code === "TIER_LOCKED"
                ? "hub.settings.notifications.failed_locked"
                : "hub.settings.failed";
        status = { tone: "refusal", text: t(message) };
      }
      // The Hub's value and rev, never a guess.
      await load(true);
    } finally {
      // The switch shows what the Hub holds, even when it refused the change.
      box.checked = rows[key]?.value === "true";
      busy = null;
    }
  }

  let seenVersion = untrack(() => branchLocale.version);
  $effect(() => {
    const version = branchLocale.version;
    untrack(() => {
      if (version === seenVersion) return;
      seenVersion = version;
      if (!busy && branchLocale.changedKeys.some((k) => READ.includes(k))) void load(true);
    });
  });

  onMount(() => {
    void load();
  });
</script>

<section class="notifications" aria-labelledby="settings-notifications-title">
  <h2 class="type-hub-title" id="settings-notifications-title">
    {t("hub.settings.notifications.title")}
  </h2>
  <p class="type-hub-body muted" data-testid="notifications-intro">
    {t("hub.settings.notifications.intro")}
  </p>
  {#if loadFailed}
    <div class="actions" data-testid="notifications-load-failed">
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
  {:else if !loading}
    <ul class="list">
      {#each NOTIFY_MESSAGES as m (m.id)}
        {@const on = rows[m.key]?.value === "true"}
        <!-- i18n-ignore -->
        <li class="message" data-testid={`notify-${m.id}`}>
          <h3 class="type-hub-label label">{t(LABELS[m.id])}</h3>
          {#if paid}
            <label class="switch type-hub-body">
              <input
                type="checkbox"
                role="switch"
                checked={on}
                disabled={busy !== null}
                aria-label={t("hub.settings.notifications.toggle", {
                  message: t(LABELS[m.id]),
                  state: t(on ? "hub.settings.notifications.on" : "hub.settings.notifications.off"),
                })}
                onchange={(e) => void flip(m.key, e.currentTarget)}
              />
              <span
                >{t(on ? "hub.settings.notifications.on" : "hub.settings.notifications.off")}</span
              >
            </label>
          {:else}
            <!-- i18n-ignore -->
            <div class="tier-lock" data-testid={`notify-lock-${m.id}`}>
              <p class="lock type-hub-label">
                <span aria-hidden="true">🔒</span>
                {t("hub.settings.notifications.locked")}
              </p>
              <p class="type-hub-label">{t("hub.settings.notifications.locked_note")}</p>
            </div>
          {/if}
          <p class="type-hub-label muted">{t("hub.settings.notifications.preview")}</p>
          <!-- i18n-ignore -->
          <blockquote class="template type-hub-body" data-testid={`notify-preview-${m.id}`}>
            {preview(m.id)}
          </blockquote>
        </li>
      {/each}
    </ul>
    <p
      class="status type-hub-label"
      class:refusal={status?.tone === "refusal"}
      role="status"
      data-testid="settings-status"
    >
      {#if status}
        {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
        <span>{status.text}</span>
      {/if}
    </p>
  {/if}
</section>

<style>
  .notifications {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  h3,
  p {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .list {
    display: grid;
    gap: var(--space-4);
    margin: 0;
    padding: 0;
    list-style: none;
    max-width: 40rem;
  }

  .message {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
  }

  .label {
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .switch {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
    color: var(--text);
    cursor: pointer;
  }

  .switch input {
    flex: none;
    width: var(--space-5);
    height: var(--space-5);
    margin: 0;
    accent-color: var(--accent);
  }

  .tier-lock {
    display: grid;
    gap: var(--space-1);
    color: var(--tier-lock-foreground);
  }

  .lock {
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .template {
    margin: 0;
    padding-left: var(--space-3);
    border-left: 3px solid var(--border-strong);
    color: var(--text);
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
    min-height: 1lh;
    color: var(--text);
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }
</style>
