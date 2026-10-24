<script lang="ts">
  import { Button, formatDateTime, t } from "@arena/ui";
  import type { AdsConsent } from "@arena/ui/generated";
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
  import {
    ADS_POLICY_VERSION,
    AD_POLICY_URL,
    KEY_ADS_CONSENT,
    adsNeeded,
    sessionStaff,
    snapshotOf,
  } from "../../../../lib/operating";

  function parseConsent(value: string): AdsConsent | null {
    try {
      const v = JSON.parse(value) as Record<string, unknown>;
      if (typeof v !== "object" || v === null || typeof v.given !== "boolean") return null;
      return {
        given: v.given,
        at: typeof v.at === "number" ? v.at : null,
        staff_id: typeof v.staff_id === "string" ? v.staff_id : null,
        policy_version: typeof v.policy_version === "number" ? v.policy_version : null,
      };
    } catch {
      return null;
    }
  }

  let server: { consent: AdsConsent; rev: number } | null = $state(null);
  let ticked = $state(false);
  let loading = $state(true);
  let loadFailed = $state(false);
  let saving = $state(false);
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let pendingKey: { given: boolean; key: string } | null = null;
  let loadSeq = 0;

  const staff = $derived(sessionStaff(page.data));
  const canEdit = $derived(hasPermission(staff, "settings.edit"));
  const needed = $derived(adsNeeded(snapshotOf(page.data)));
  const changed = $derived.by(() => server !== null && ticked !== server.consent.given);
  const recorded = $derived.by(() => {
    const c = server?.consent;
    if (!c || c.at === null) return null;
    const date = formatDateTime(c.at, branchLocale.current?.tz);
    return c.given
      ? t("hub.settings.ads.given", { date })
      : t("hub.settings.ads.withdrawn", { date });
  });

  async function load(keepTick = false) {
    const seq = ++loadSeq;
    loading = true;
    try {
      const result = await loadSettings();
      if (seq !== loadSeq) return;
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const entry =
        result.kind === "ok" ? result.settings.find((s) => s.key === KEY_ADS_CONSENT) : undefined;
      const consent = entry ? parseConsent(entry.value) : null;
      if (!entry || !consent) {
        loadFailed = true;
        return;
      }
      loadFailed = false;
      server = { consent, rev: entry.rev };
      if (!keepTick) ticked = consent.given;
    } finally {
      if (seq === loadSeq) loading = false;
    }
  }

  async function save() {
    if (!canEdit || saving || loading || !changed || !server) return;
    const given = ticked;
    if (pendingKey?.given !== given) pendingKey = { given, key: newIdempotencyKey() };
    saving = true;
    status = null;
    try {
      const outcome = await submitIntent(
        { kind: "ads.consent", given, policy_version: ADS_POLICY_VERSION },
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
      const code = outcome.kind === "error" ? outcome.error.code : null;
      if (code === "STALE") {
        pendingKey = null;
        status = { tone: "refusal", message: "hub.settings.branch.stale" };
        await load(true);
      } else if (code === "PERMISSION_DENIED") {
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
      if (!saving && branchLocale.changedKeys.includes(KEY_ADS_CONSENT)) void load(changed);
    });
  });

  onMount(() => {
    void load();
  });
</script>

<section class="ads" aria-labelledby="settings-ads-title">
  <h2 class="type-hub-title" id="settings-ads-title">{t("hub.settings.ads.title")}</h2>
  {#if !needed}
    <p class="note type-hub-body" data-testid="ads-not-needed">
      {t("hub.settings.ads.not_needed")}
    </p>
  {:else if loadFailed}
    <div class="actions" data-testid="ads-load-failed">
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
    <p class="intro type-hub-body">{t("hub.settings.ads.intro")}</p>
    <form
      class="form"
      data-testid="settings-ads"
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <p class="text type-hub-body" id="ads-consent-text" data-testid="ads-text">
        {t("hub.settings.ads.text")}
      </p>
      <p class="type-hub-body">
        <a class="link ui-interactive" href={AD_POLICY_URL} target="_blank" rel="noreferrer"
          >{t("hub.settings.ads.policy")}</a
        >
      </p>
      <label class="option type-hub-body">
        <input
          type="checkbox"
          bind:checked={ticked}
          disabled={!canEdit || saving || loading}
          aria-describedby="ads-consent-text"
        />
        <span>{t("hub.settings.ads.consent")}</span>
      </label>
      <p class="note type-hub-label" data-testid="ads-recorded">
        {recorded ?? t("hub.settings.ads.missing")}
      </p>
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
          data-testid="ads-status"
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
  .ads {
    display: grid;
    gap: var(--space-3);
  }

  h2,
  p {
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

  .text,
  .note {
    color: var(--text);
  }

  .link {
    --ui-bg: transparent;
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    color: var(--text);
    text-decoration: underline;
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

  .option:has(input:disabled) {
    opacity: 0.4;
    cursor: not-allowed;
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
