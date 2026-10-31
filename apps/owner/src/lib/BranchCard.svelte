<script lang="ts">
  import { Button, PairingCode, StatusBadge, TextField, TierLock, formatTime, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { OwnerBranch, OwnerCommandView, PairingCodeIssued } from "@arena/ui/generated";
  import { load, send, setupUrl } from "./api";
  import CommandRow from "./CommandRow.svelte";
  import {
    POLL_MS,
    backupNow,
    cmd,
    denyKey,
    drainQueue,
    isWaiting,
    localRows,
    refreshCommands,
    releaseKey,
    revokeHub,
  } from "./commands.svelte";
  import { age, refresh } from "./shell.svelte";
  import { ulid } from "./ulid";

  let { branch }: { branch: OwnerBranch } = $props();

  const id = $props.id();
  const TIERS = {
    free: "owner.tier.free",
    basic: "owner.tier.basic",
    premium: "owner.tier.premium",
  } as const;

  let editing = $state(false);
  let name = $state("");
  let timezone = $state("");
  let zones: string[] = $state([]);
  let error: MessageKey | null = $state(null);
  let issued: PairingCodeIssued | null = $state(null);
  let busy = $state(false);
  let replacing = $state(false);
  let replaceKey = ulid();
  let replaceError = $state(false);
  let replaced: OwnerCommandView | null = $state(null);
  /** One idempotency key per request and decision, so a retry of the same tap is the same post. */
  const keyKeys = new Map<string, string>();
  let keyBusy = $state(false);
  let keyError: MessageKey | null = $state(null);
  let keyDecided: OwnerCommandView | null = $state(null);
  let backupKey = ulid();
  let backing = $state(false);
  let backupError: string | null = $state(null);
  /** The lowest command id this card started (0: queued on this device); null before the first tap. */
  let followed: number | null = $state(null);
  let now = $state(Date.now());

  const seen = $derived.by(() => {
    const at = branch.hub.last_seen_at;
    if (!branch.hub.paired) return t("owner.branches.hub.none");
    if (!at) return t("owner.branches.hub.new");
    const [key, n] = age(new Date(at).toISOString());
    return t("owner.branches.hub.seen", { age: t(key, { n }) });
  });

  const paid = $derived(branch.tier !== "free");
  const ago = (ms: number) => {
    const [key, n] = age(new Date(ms).toISOString());
    return t(key, { n });
  };
  /** The backup line (Story 9.17): the failure replaces the good state while the Hub reports one. */
  const backupLine = $derived.by(() => {
    const b = branch.backup;
    if (!b) return t("owner.branches.backup.none");
    if (b.failed_at !== undefined)
      return t("owner.branches.backup.failed", { time: formatTime(b.failed_at, branch.timezone) });
    return t("owner.branches.backup.line", {
      upload:
        b.last_upload_at === undefined
          ? t("owner.branches.backup.upload_none")
          : t("owner.branches.backup.upload", { age: ago(b.last_upload_at) }),
      snapshot:
        b.last_snapshot_at === undefined
          ? t("owner.branches.backup.snapshot_none")
          : t("owner.branches.backup.snapshot", { age: ago(b.last_snapshot_at) }),
      pending:
        b.pending_batches === 1
          ? t("owner.branches.backup.pending_one")
          : t("owner.branches.backup.pending", { n: b.pending_batches }),
    });
  });
  const backupFailed = $derived(branch.backup?.failed_at !== undefined);

  /** The newest `backup.now` this card started, or what a Retry of it made after. */
  const backupRow = $derived.by(() => {
    const from = followed;
    if (from === null) return null;
    return (
      [...localRows(), ...cmd.rows].find(
        (r) => r.kind === "backup.now" && r.branch_id === branch.id && (r.id === 0 || r.id >= from),
      ) ?? null
    );
  });
  const backupWaiting = $derived(!!backupRow && isWaiting(backupRow));

  // While the command is not finished the row follows it, and the card reads the branch again once
  // it is, so the backup line is the Hub's latest.
  let wasWaiting = false;
  $effect(() => {
    const waitingNow = backupWaiting;
    if (wasWaiting && !waitingNow) void refresh();
    wasWaiting = waitingNow;
    if (!waitingNow) return;
    const tick = async () => {
      now = Date.now();
      if (cmd.queued.some((q) => q.state === "queued")) await drainQueue();
      await refreshCommands();
    };
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void tick();
    }, POLL_MS);
    return () => clearInterval(timer);
  });

  async function backUp() {
    backing = true;
    backupError = null;
    const res = await backupNow(branch.id, backupKey);
    backing = false;
    if (res.state === "limited") backupError = res.message;
    else if (res.state === "failed") backupError = t("owner.branches.backup.error");
    else if (res.state === "sent" || res.state === "queued") {
      backupKey = ulid();
      now = Date.now();
      followed = Math.min(followed ?? res.id, res.id);
    }
  }

  let standbyIssued: PairingCodeIssued | null = $state(null);
  let standbyBusy = $state(false);
  let standbyError: MessageKey | null = $state(null);

  /** The standby PC (Story 9.18): a plan with the `standby` flag and a paired Windows main Hub. */
  const standbyOn = $derived(branch.flags.includes("standby"));
  const standbyPaired = $derived(branch.standby?.paired === true);
  const standbySeen = $derived.by(() => {
    const at = branch.standby?.last_seen_at;
    if (at === undefined) return t("owner.branches.standby.paired");
    const [key, n] = age(new Date(at).toISOString());
    return t("owner.branches.standby.seen", { age: t(key, { n }) });
  });

  async function addStandby() {
    standbyBusy = true;
    standbyError = null;
    const res = await send<PairingCodeIssued>("POST", "/wizard/install-hub/standby-code", {
      branch_id: branch.id,
    });
    standbyBusy = false;
    if (res.ok) {
      standbyIssued = res.data;
      await refresh();
      return;
    }
    standbyError =
      res.code === "STANDBY_EXISTS"
        ? "owner.branches.standby.exists"
        : res.code === "STEP_NOT_READY"
          ? "owner.branches.standby.needs_hub"
          : res.code === "PLATFORM_UNSUPPORTED"
            ? "owner.branches.standby.needs_windows"
            : "owner.branches.error.generic";
    if (res.code === "STANDBY_EXISTS" || res.code === "TIER_LOCKED") await refresh();
  }

  async function edit() {
    name = branch.name;
    timezone = branch.timezone;
    error = null;
    editing = true;
    const res = await load<string[]>("/meta/timezones");
    if (res && res !== "signed-out") zones = res.data;
  }

  async function save(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    const res = await send("PATCH", `/branches/${encodeURIComponent(branch.id)}`, {
      name,
      timezone,
    });
    busy = false;
    if (res.ok) {
      editing = false;
      await refresh();
    } else {
      error =
        res.code === "VALIDATION_FAILED"
          ? res.field === "timezone"
            ? "owner.branches.error.timezone"
            : "owner.branches.error.name"
          : "owner.branches.error.generic";
    }
  }

  async function pair() {
    busy = true;
    error = null;
    const res = await send<PairingCodeIssued>("POST", "/wizard/install-hub/code", {
      branch_id: branch.id,
    });
    busy = false;
    if (res.ok) issued = res.data;
    else
      error =
        res.code === "STEP_NOT_READY"
          ? "owner.branches.error.not_ready"
          : "owner.branches.error.generic";
  }

  function askReplace() {
    replaceKey = ulid();
    replaceError = false;
    replacing = true;
  }

  async function answer(decision: "approve" | "deny") {
    const ask = branch.key_request;
    if (!ask) return;
    const slot = `${ask.id}:${decision}`; // i18n-ignore
    const key = keyKeys.get(slot) ?? ulid();
    keyKeys.set(slot, key);
    keyBusy = true;
    keyError = null;
    const res = await (decision === "approve" ? releaseKey : denyKey)(branch.id, key, ask.id);
    keyBusy = false;
    if (res === "declined") return;
    if (res === "failed" || res === "refused") {
      keyError = res === "failed" ? "owner.branches.key.failed" : "owner.branches.key.refused";
      if (res === "refused") await refresh();
      return;
    }
    keyDecided = res;
    await refresh();
  }

  async function replace() {
    busy = true;
    replaceError = false;
    const res = await revokeHub(branch.id, replaceKey);
    if (res === "declined") {
      busy = false;
      return;
    }
    if (res === "failed") {
      busy = false;
      replaceError = true;
      return;
    }
    replaced = res;
    replacing = false;
    await refresh();
    await pair();
  }
</script>

<li class="card">
  <h2 class="type-hub-label">{branch.name}</h2>
  <p>
    {t("owner.branches.code", { code: branch.branch_code })} ·
    {t("owner.branches.plan", { tier: t(TIERS[branch.tier]) })} ·
    {t(branch.live ? "owner.branches.state.live" : "owner.branches.state.setup")}
  </p>
  <p>{seen}</p>
  {#if branch.hub.paired && branch.hub.platform}
    <p data-testid="hub-platform">
      {t("owner.branches.hub.platform", {
        platform: t(
          branch.hub.platform === "android"
            ? "owner.branches.hub.platform.android"
            : "owner.branches.hub.platform.windows",
        ),
      })}
    </p>
  {/if}
  {#if paid}
    <p data-testid="backup-line" data-failed={backupFailed}>{backupLine}</p>
  {/if}
  {#if branch.waiting}<StatusBadge state="warning" label="owner.branches.waiting" />{/if}
  {#if editing}
    <form onsubmit={save}>
      <TextField label="owner.branches.field.name" bind:value={name} required />
      <label class="label" for="{id}-tz">{t("owner.branches.field.timezone")}</label>
      <select id="{id}-tz" class="ui-interactive select" bind:value={timezone}>
        {#each zones.length ? zones : [timezone] as z (z)}<option value={z}>{z}</option>{/each}
      </select>
      <p class="hint">{t("owner.branches.tz_note")}</p>
      {#if error}<p role="alert">{t(error)}</p>{/if}
      <div class="actions">
        <Button
          label="owner.branches.save"
          type="submit"
          disabled={busy}
          disabledReason="owner.loading"
        />
        <Button
          label="owner.branches.cancel"
          variant="secondary"
          onclick={() => (editing = false)}
        />
      </div>
    </form>
  {:else}
    <div class="actions">
      <Button label="owner.branches.edit" variant="secondary" onclick={edit} />
      {#if paid}
        <Button
          label="owner.branches.backup.now"
          variant="secondary"
          onclick={backUp}
          disabled={backing || backupWaiting}
          disabledReason={backing
            ? "owner.branches.backup.working"
            : "owner.branches.backup.waiting"}
        />
      {:else}
        <TierLock tier="Basic" branch={branch.id} />
      {/if}
      {#if branch.hub.paired && !replacing}
        <Button label="owner.branches.replace" variant="secondary" onclick={askReplace} />
      {/if}
      {#if !branch.hub.paired && !issued}
        <Button
          label="owner.branches.pair"
          variant="secondary"
          onclick={pair}
          disabled={busy}
          disabledReason="owner.branches.pair_wait"
        />
      {/if}
      {#if !branch.live}
        <a class="link" href={setupUrl(branch.id)}>{t("owner.branches.continue")}</a>
      {/if}
    </div>
    {#if backupError}<p role="alert" data-testid="backup-error">{backupError}</p>{/if}
    {#if backupRow}
      <ul class="replaced">
        <CommandRow row={backupRow} {now} />
      </ul>
    {/if}
    <div class="standby" data-testid="standby">
      <p class="type-hub-label">{t("owner.branches.standby")}</p>
      {#if !standbyOn}
        <TierLock tier="Premium" branch={branch.id} />
      {:else if branch.hub.platform === "android"}
        <p>{t("owner.branches.standby.needs_windows")}</p>
      {:else if standbyPaired}
        <p data-testid="standby-paired">{standbySeen}</p>
      {:else if !branch.hub.paired}
        <p class="hint">{t("owner.branches.standby.needs_hub")}</p>
      {:else}
        {#if standbyIssued}
          <PairingCode
            code={standbyIssued.code}
            expiresAt={standbyIssued.expires_at}
            serverNow={standbyIssued.server_now}
            onregenerate={addStandby}
            regenerating={standbyBusy}
          />
          <p class="hint">{t("owner.branches.standby.note")}</p>
        {:else}
          {#if branch.standby?.code}
            <p data-testid="standby-waiting">
              {t("owner.branches.standby.waiting", {
                time: formatTime(branch.standby.code.expires_at, branch.timezone),
              })}
            </p>
          {/if}
          <div class="actions">
            <Button
              label={branch.standby?.code
                ? "owner.branches.standby.new"
                : "owner.branches.standby.add"}
              variant="secondary"
              onclick={addStandby}
              disabled={standbyBusy}
              disabledReason="owner.branches.standby.working"
            />
          </div>
        {/if}
        {#if standbyError}<p role="alert">{t(standbyError)}</p>{/if}
      {/if}
    </div>
    {#if branch.key_request}
      {@const ask = branch.key_request}
      {@const [ageKey, ageN] = age(new Date(ask.requested_at).toISOString())}
      <div class="replace" role="group" aria-labelledby="{id}-key" data-testid="key-request">
        <p id="{id}-key" class="type-hub-label">
          {t("owner.branches.key.title", {
            pc: ask.pc_name ?? t("owner.branches.key.pc_fallback"),
          })}
        </p>
        <p class="hint">{t("owner.branches.key.asked", { age: t(ageKey, { n: ageN }) })}</p>
        <p>{t("owner.branches.key.note")}</p>
        {#if keyError}<p role="alert">{t(keyError)}</p>{/if}
        <div class="actions">
          <Button
            label="owner.branches.key.approve"
            onclick={() => answer("approve")}
            disabled={keyBusy}
            disabledReason="owner.branches.key.working"
          />
          <Button
            label="owner.branches.key.deny"
            variant="secondary"
            onclick={() => answer("deny")}
            disabled={keyBusy}
            disabledReason="owner.branches.key.working"
          />
        </div>
      </div>
    {/if}
    {#if keyDecided}
      <ul class="replaced">
        <CommandRow row={keyDecided} now={Date.now()} />
      </ul>
    {/if}
    {#if replacing}
      <div class="replace" role="group" aria-labelledby="{id}-replace">
        <p id="{id}-replace" class="type-hub-label">{t("owner.branches.replace.title")}</p>
        <p>{t("owner.branches.replace.warning")}</p>
        {#if replaceError}<p role="alert">{t("owner.branches.replace.failed")}</p>{/if}
        <div class="actions">
          <Button
            label="owner.branches.replace.confirm"
            onclick={replace}
            disabled={busy}
            disabledReason="owner.branches.replace.working"
          />
          <Button
            label="owner.branches.cancel"
            variant="secondary"
            onclick={() => (replacing = false)}
            disabled={busy}
            disabledReason="owner.branches.replace.working"
          />
        </div>
      </div>
    {/if}
    {#if error}<p role="alert">{t(error)}</p>{/if}
    {#if replaced}
      <ul class="replaced">
        <CommandRow row={replaced} now={Date.now()} />
      </ul>
    {/if}
    {#if issued && !branch.hub.paired}
      {#if replaced}<p>{t("owner.branches.replace.done")}</p>{/if}
      <PairingCode
        code={issued.code}
        expiresAt={issued.expires_at}
        serverNow={issued.server_now}
        onregenerate={pair}
        regenerating={busy}
      />
    {/if}
  {/if}
</li>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background-color: var(--card-background);
  }

  h2,
  p {
    margin: 0;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .label {
    font-weight: var(--type-hub-title-weight);
  }

  .select {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--text-field-background);
    color: var(--text-field-foreground);
    font: inherit;
  }

  .replace {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
  }

  .standby {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .replaced {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .hint {
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }

  .link {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    color: var(--accent-text);
    text-decoration: underline;
  }
</style>
