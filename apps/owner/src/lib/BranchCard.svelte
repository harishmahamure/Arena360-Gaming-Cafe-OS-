<script lang="ts">
  import { Button, PairingCode, StatusBadge, TextField, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { OwnerBranch, OwnerCommandView, PairingCodeIssued } from "@arena/ui/generated";
  import { load, send, setupUrl } from "./api";
  import CommandRow from "./CommandRow.svelte";
  import { revokeHub } from "./commands.svelte";
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

  const seen = $derived.by(() => {
    const at = branch.hub.last_seen_at;
    if (!branch.hub.paired) return t("owner.branches.hub.none");
    if (!at) return t("owner.branches.hub.new");
    const [key, n] = age(new Date(at).toISOString());
    return t("owner.branches.hub.seen", { age: t(key, { n }) });
  });

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
