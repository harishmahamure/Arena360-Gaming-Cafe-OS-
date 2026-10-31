<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { HandoverView } from "@arena/ui/generated";
  import { invalidate } from "$app/navigation";
  import { importMove, joinMove, useMoveFile } from "./handover";
  import { SNAPSHOT_DEPENDENCY } from "./snapshot";

  /**
   * The Windows Hub of a Hub move, paired with the owner's code and waiting for the Android Hub
   * (Story 10.16). It asks for the tablet's address and the 6-digit code on its screen (Story
   * 10.17), pulls the snapshot (or takes a file) and says when it holds it. Once the owner has
   * released the branch key to this PC it imports the snapshot (Story 10.18) and restarts.
   */
  let {
    branchName,
    received = null,
    keyReleased = false,
  }: { branchName: string; received?: HandoverView | null; keyReleased?: boolean } = $props();

  let address = $state("");
  let code = $state("");
  let busy = $state(false);
  /** The Hub's own words for why the move did not start (they name what to do next). */
  let problem = $state<string | null>(null);
  let unreachable = $state(false);
  let path = $state("");
  let importing = $state(false);

  const fileReady = $derived(path.trim().length > 0 && !busy);

  const ready = $derived(address.trim().length > 0 && /^\d{6}$/.test(code.trim()) && !busy);

  async function join(event: SubmitEvent) {
    event.preventDefault();
    if (!ready) return;
    busy = true;
    problem = null;
    unreachable = false;
    try {
      const result = await joinMove(address.trim(), code.trim());
      if (result.kind === "ok") {
        await invalidate(SNAPSHOT_DEPENDENCY);
      } else if (result.kind === "refused") {
        problem = result.error.message;
      } else {
        unreachable = true;
      }
    } finally {
      busy = false;
    }
  }

  async function useFile(event: SubmitEvent) {
    event.preventDefault();
    if (!fileReady) return;
    busy = true;
    problem = null;
    unreachable = false;
    try {
      const result = await useMoveFile(path.trim());
      if (result.kind === "ok") {
        await invalidate(SNAPSHOT_DEPENDENCY);
      } else if (result.kind === "refused") {
        problem = result.error.message;
      } else {
        unreachable = true;
      }
    } finally {
      busy = false;
    }
  }

  async function importBranch() {
    if (!keyReleased || importing) return;
    importing = true;
    problem = null;
    unreachable = false;
    const result = await importMove();
    if (result.kind === "ok") {
      // The Hub restarts onto the imported branch; the page reloads its snapshot by itself.
      return;
    }
    importing = false;
    if (result.kind === "refused") problem = result.error.message;
    else unreachable = true;
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.migration_wait.title") })}</title>
</svelte:head>

<section class="wait" aria-labelledby="wait-title" data-testid="hub-migration-wait">
  <h1 class="type-hub-numeric" id="wait-title">{t("hub.migration_wait.title")}</h1>
  <p class="type-hub-body" data-testid="wait-branch">
    {t("hub.migration_wait.branch")}: <strong>{branchName}</strong>
  </p>
  {#if received?.phase === "received"}
    <p class="type-hub-body" role="status" data-testid="handover-received">
      {t("hub.migration_wait.received", { seq: received.last_seq ?? 0 })}
    </p>
    <p class="type-hub-body" role="status" data-testid="handover-key">
      {keyReleased ? t("hub.migration_wait.key_released") : t("hub.migration_wait.key_waiting")}
    </p>
    <Button
      label="hub.migration_wait.import"
      onclick={importBranch}
      disabled={!keyReleased || importing}
      disabledReason={importing
        ? "hub.migration_wait.importing"
        : "hub.migration_wait.import_needs"}
    />
    <div role="status" aria-live="polite">
      {#if importing}
        <p class="type-hub-label muted" data-testid="handover-importing">
          {t("hub.migration_wait.importing")}
        </p>
      {:else if problem}
        <p class="type-hub-label" data-testid="handover-problem">
          <span aria-hidden="true">⚠</span>
          {problem}
        </p>
      {:else if unreachable}
        <p class="type-hub-label" data-testid="handover-problem">
          <span aria-hidden="true">⚠</span>
          {t("hub.migration_wait.unreachable")}
        </p>
      {/if}
    </div>
  {:else}
    <p class="type-hub-body">{t("hub.migration_wait.steps")}</p>
    <form class="join" onsubmit={join} data-testid="handover-join">
      <label class="field">
        <span class="type-hub-label">{t("hub.migration_wait.address_label")}</span>
        <input
          class="type-hub-body"
          type="text"
          inputmode="decimal"
          autocomplete="off"
          spellcheck="false"
          bind:value={address}
          data-testid="handover-address-input"
        />
      </label>
      <label class="field">
        <span class="type-hub-label">{t("hub.migration_wait.code_label")}</span>
        <input
          class="type-hub-body"
          type="text"
          inputmode="numeric"
          autocomplete="off"
          maxlength="6"
          bind:value={code}
          data-testid="handover-code-input"
        />
      </label>
      <Button
        label="hub.migration_wait.join"
        type="submit"
        disabled={!ready}
        disabledReason={busy ? "hub.migration_wait.joining" : "hub.migration_wait.join_needs"}
      />
    </form>
    <form class="join" onsubmit={useFile} data-testid="handover-file">
      <label class="field">
        <span class="type-hub-label">{t("hub.migration_wait.file_label")}</span>
        <input
          class="type-hub-body"
          type="text"
          autocomplete="off"
          spellcheck="false"
          bind:value={path}
          data-testid="handover-file-input"
        />
      </label>
      <Button
        label="hub.migration_wait.file_use"
        type="submit"
        disabled={!fileReady}
        disabledReason={busy ? "hub.migration_wait.joining" : "hub.migration_wait.file_needs"}
      />
    </form>
    <div role="status" aria-live="polite">
      {#if busy}
        <p class="type-hub-label muted">{t("hub.migration_wait.joining")}</p>
      {:else if problem}
        <p class="type-hub-label" data-testid="handover-problem">
          <span aria-hidden="true">⚠</span>
          {problem}
        </p>
      {:else if unreachable}
        <p class="type-hub-label" data-testid="handover-problem">
          <span aria-hidden="true">⚠</span>
          {t("hub.migration_wait.unreachable")}
        </p>
      {/if}
    </div>
  {/if}
  <p class="type-hub-body">{t("hub.migration_wait.serves_nothing")}</p>
</section>

<style>
  .wait {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 60ch;
  }

  h1,
  p {
    margin: 0;
  }

  .join {
    display: grid;
    gap: var(--space-3);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  input {
    min-block-size: var(--target-min, 2.75rem);
    padding-inline: var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface);
    color: var(--text);
  }

  .muted {
    color: var(--text-muted);
  }
</style>
