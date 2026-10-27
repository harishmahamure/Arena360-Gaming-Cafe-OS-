<script lang="ts">
  import { Button, InlinePin, billingEnabled, formatDateTime, t } from "@arena/ui";
  import type { AdRating, AdsConsent } from "@arena/ui/generated";
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
  } from "../../../../lib/auth";
  import { adRatingOf } from "../../../../lib/ads";
  import { branchLocale } from "../../../../lib/locale.svelte";
  import {
    ADS_POLICY_VERSION,
    AD_POLICY_URL,
    KEY_ADS_CONSENT,
    KEY_ADS_RATING,
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

  /** The ratings a branch can choose, loosest last; MA is never offered. */
  const RATINGS: readonly (readonly [AdRating, MessageKey])[] = [
    ["G", "hub.settings.ads.rating.g"],
    ["PG", "hub.settings.ads.rating.pg"],
    ["T", "hub.settings.ads.rating.t"],
  ];
  const EXCLUDED: readonly MessageKey[] = [
    "hub.settings.ads.excluded.adult",
    "hub.settings.ads.excluded.gambling",
    "hub.settings.ads.excluded.alcohol",
  ];

  let server: { consent: AdsConsent; rev: number } | null = $state(null);
  let ratingServer: { value: AdRating; rev: number } | null = $state(null);
  let rating: AdRating = $state("PG");
  let ratingSaving = $state(false);
  let approving = $state(false);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let ratingStatus: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let ratingIdem: { value: AdRating; key: string } | null = null;
  let ratingWrap: HTMLElement | undefined = $state();
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
  const needsApproval = $derived(!hasPermission(staff, "settings.edit"));
  const ratingChanged = $derived.by(() => ratingServer !== null && rating !== ratingServer.value);
  const ratingLocked = $derived(staff === null || ratingSaving || approving);
  const ratingButton = $derived(ratingWrap?.querySelector("button") ?? null);
  const recorded = $derived.by(() => {
    const c = server?.consent;
    if (!c || c.at === null) return null;
    const date = formatDateTime(c.at, branchLocale.current?.tz);
    const by = c.staff_id ?? "-";
    return c.given
      ? t("hub.settings.ads.given", { date, staff: by })
      : t("hub.settings.ads.withdrawn", { date, staff: by });
  });

  async function load(keepTick = false, keepRating = false) {
    const seq = ++loadSeq;
    loading = true;
    try {
      const result = await loadSettings();
      if (seq !== loadSeq) return;
      if (result.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      const entry =
        result.kind === "ok" ? result.settings.find((s) => s.key === KEY_ADS_CONSENT) : undefined;
      const consent = entry ? parseConsent(entry.value) : null;
      const ratingEntry =
        result.kind === "ok" ? result.settings.find((s) => s.key === KEY_ADS_RATING) : undefined;
      if (!entry || !consent || !ratingEntry) {
        loadFailed = true;
        return;
      }
      loadFailed = false;
      server = { consent, rev: entry.rev };
      if (!keepTick) ticked = consent.given;
      // A stored MA or unknown value reads as PG.
      ratingServer = { value: adRatingOf(ratingEntry.value), rev: ratingEntry.rev };
      if (!keepRating) rating = ratingServer.value;
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
        await load(false, ratingChanged);
        return;
      }
      const code = outcome.kind === "error" ? outcome.error.code : null;
      if (code === "STALE") {
        pendingKey = null;
        status = { tone: "refusal", message: "hub.settings.branch.stale" };
        await load(true, ratingChanged);
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

  async function approve(pin: string) {
    const outcome = await elevate([{ intent_kind: "settings.set", key: KEY_ADS_RATING }], pin);
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
    void saveRating(outcome.ticket);
    return { ok: true } as const;
  }

  /** Saves the content rating: `settings.set`, approved with a manager PIN when staff lack the permission. */
  async function saveRating(ticket?: string) {
    if (staff === null || ratingSaving || loading || !ratingChanged || !ratingServer) {
      if (ticket) void dropElevation(ticket);
      return;
    }
    if (needsApproval && !ticket) {
      approvalPrompt = "ui.pin.prompt";
      approving = true;
      return;
    }
    ratingSaving = true;
    ratingStatus = null;
    let again = false;
    let done = false;
    try {
      const value = rating;
      if (ratingIdem?.value !== value) ratingIdem = { value, key: newIdempotencyKey() };
      const outcome = await submitIntent(
        { kind: "settings.set", key: KEY_ADS_RATING, value },
        ratingIdem.key,
        ratingServer.rev,
        { elevationTicket: ticket },
      );
      if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
      if (outcome.kind === "applied") {
        ratingIdem = null;
        done = true;
        await load(changed);
        ratingStatus = { tone: "ok", message: "hub.settings.saved" };
        return;
      }
      const code = outcome.kind === "error" ? outcome.error.code : null;
      if (code === "STALE") {
        ratingIdem = null;
        ratingStatus = { tone: "refusal", message: "hub.settings.branch.stale" };
        await load(changed, true);
      } else if (code === "PIN_REQUIRED") {
        again = true;
      } else if (code === "PERMISSION_DENIED") {
        ratingIdem = null;
        ratingStatus = { tone: "refusal", message: "hub.settings.branch.manager_only" };
      } else {
        ratingStatus = { tone: "refusal", message: "hub.settings.failed" };
      }
    } finally {
      ratingSaving = false;
      if (ticket && !again && !done) void dropElevation(ticket);
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
      const keys = branchLocale.changedKeys;
      if (
        !saving &&
        !ratingSaving &&
        (keys.includes(KEY_ADS_CONSENT) || keys.includes(KEY_ADS_RATING))
      ) {
        void load(changed, ratingChanged);
      }
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

    <div class="excluded" data-testid="ads-excluded">
      <h3 class="type-hub-label" id="ads-excluded-title">{t("hub.settings.ads.excluded.title")}</h3>
      <ul class="type-hub-body" aria-labelledby="ads-excluded-title">
        {#each EXCLUDED as item (item)}<li>{t(item)}</li>{/each}
      </ul>
    </div>

    <form
      class="form"
      data-testid="settings-ads-rating"
      novalidate
      onsubmit={(e) => {
        e.preventDefault();
        void saveRating();
      }}
    >
      <fieldset class="group" aria-describedby="ads-rating-hint">
        <legend class="label type-hub-label">{t("hub.settings.ads.rating.title")}</legend>
        <div class="options">
          {#each RATINGS as [value, label] (value)}
            <label class="option type-hub-body">
              <input
                type="radio"
                name="ads-rating"
                {value}
                bind:group={rating}
                disabled={ratingLocked}
              />
              <span>{t(label)}</span>
            </label>
          {/each}
        </div>
        <p class="hint type-hub-label" id="ads-rating-hint">{t("hub.settings.ads.rating.hint")}</p>
      </fieldset>
      <div class="actions">
        <span bind:this={ratingWrap}>
          <Button
            label="hub.settings.save"
            type="submit"
            disabled={ratingLocked || loading || !ratingChanged}
            disabledReason={approving
              ? "hub.settings.branch.needs_approval"
              : ratingSaving
                ? "hub.settings.saving"
                : loading
                  ? "hub.settings.loading"
                  : "hub.settings.branch.no_changes"}
          />
        </span>
        {#if needsApproval && !approving}
          <p class="note type-hub-label" data-testid="ads-rating-needs-approval">
            {t("hub.settings.branch.needs_approval")}
          </p>
        {/if}
        <p
          class="status type-hub-label"
          class:refusal={ratingStatus?.tone === "refusal"}
          role="status"
          data-testid="ads-rating-status"
        >
          {#if ratingStatus}
            {#if ratingStatus.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
            <span>{t(ratingStatus.message)}</span>
          {/if}
        </p>
      </div>
      {#if approving}
        <div class="approval" data-testid="ads-rating-approval">
          <InlinePin
            message={approvalPrompt}
            onsubmit={approve}
            oncancel={() => (approving = false)}
            returnFocusTo={ratingButton}
          />
        </div>
      {/if}
    </form>

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
    <p class="keep-free type-hub-body" data-testid="ads-keep-free">
      <span>{t("hub.settings.ads.keep_free")}</span>
      {#if billingEnabled}
        <a class="link ui-interactive" href="/account/subscription">{t("staff.tier.upgrade")}</a>
        <span>{t("hub.settings.ads.remove_them")}</span>
      {:else}
        <span>{t("staff.tier.soon")}</span>
      {/if}
    </p>
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

  .excluded {
    display: grid;
    gap: var(--space-1);
    max-width: 40rem;
  }

  .excluded h3 {
    margin: 0;
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .excluded ul {
    margin: 0;
    padding-inline-start: var(--space-5);
    color: var(--text);
  }

  .group {
    display: grid;
    gap: var(--space-2);
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
    display: grid;
    gap: var(--space-1);
  }

  .hint {
    color: var(--text-muted);
  }

  .keep-free {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    color: var(--text);
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
