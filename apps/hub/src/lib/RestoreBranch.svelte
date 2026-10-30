<script lang="ts">
  import { onMount } from "svelte";
  import type { RestoreFetchView, RestoreRebuildView } from "@arena/ui/generated";
  import { Button, t } from "@arena/ui";
  import { formatBytes } from "./media-cache";
  import {
    cancelRestoreCloud,
    cancelRestoreRebuild,
    restoreCloudPartial,
    restoreCloudView,
    restoreRebuildView,
    startRestoreCloud,
    startRestoreRebuild,
    type RestoreCloudResult,
    type RestoreRebuildResult,
  } from "./restore-cloud";

  /**
   * `released`: the owner released the branch key to this PC and it holds the key (Story 9.10).
   * `tier`: the cloud backup is a paid feature (Story 9.12); on Free its option is not shown.
   */
  let {
    branchName,
    released = false,
    tier = null,
  }: { branchName: string; released?: boolean; tier?: string | null } = $props();

  const POLL_MS = 1000;
  let view = $state<RestoreFetchView | null>(null);
  let confirming = $state(false);
  /** A refusal or an unreachable Hub service from the last action. */
  let problem = $state<string | null>(null);
  let busy = $state(false);
  /** The rebuild from the downloaded backup (Story 9.13), once it has started. */
  let rebuild = $state<RestoreRebuildView | null>(null);
  let rebuildConfirming = $state(false);

  function takeRebuild(result: RestoreRebuildResult) {
    if (result.kind === "ok") {
      rebuild = result.view;
      problem = null;
    } else {
      problem = result.kind === "refused" ? result.code : "HUB_UNREACHABLE";
    }
  }

  async function actRebuild(run: () => Promise<RestoreRebuildResult>) {
    busy = true;
    try {
      takeRebuild(await run());
    } finally {
      busy = false;
    }
    rebuildConfirming = false;
  }

  function take(result: RestoreCloudResult) {
    if (result.kind === "ok") {
      view = result.view;
      problem = null;
    } else {
      problem = result.kind === "refused" ? result.code : "HUB_UNREACHABLE";
    }
  }

  async function refresh() {
    take(await restoreCloudView());
  }

  async function act(run: () => Promise<RestoreCloudResult>) {
    busy = true;
    try {
      take(await run());
    } finally {
      busy = false;
    }
    confirming = false;
  }

  onMount(() => {
    void refresh();
    // A rebuild that was running when this screen opened (the Hub restarts onto it when done).
    void restoreRebuildView().then(takeRebuild);
  });

  // While the rebuild runs, its progress is read once a second.
  $effect(() => {
    if (rebuild?.phase !== "replaying" && rebuild?.phase !== "checking") return;
    const timer = setInterval(() => void restoreRebuildView().then(takeRebuild), POLL_MS);
    return () => clearInterval(timer);
  });

  // While files come down, the progress is read once a second.
  $effect(() => {
    if (view?.phase !== "downloading") return;
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  });

  const showCloud = $derived(tier !== "free");
  const gap = $derived(view?.gap ?? null);
  const ready = $derived(view?.ready ?? null);
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.restore_pending.title") })}</title>
</svelte:head>

<section class="restore" aria-labelledby="restore-title" data-testid="hub-restore-pending">
  <h1 class="type-hub-numeric" id="restore-title">{t("hub.restore_pending.title")}</h1>
  <p class="type-hub-body" data-testid="restore-branch">
    {t("hub.restore_pending.branch")}: <strong>{branchName}</strong>
  </p>
  <p class="type-hub-body">{t("hub.restore_pending.replaces")}</p>
  <p class="type-hub-body">{t("hub.restore_pending.serves_nothing")}</p>
  {#if released}
    <p class="type-hub-body" data-testid="restore-released">{t("hub.restore_pending.released")}</p>
  {/if}
  <!-- The options are off until the owner releases the branch key (Story 9.10). -->
  <div class="options">
    <Button
      label="hub.restore_pending.option"
      disabled={!released}
      disabledReason="hub.restore_pending.waiting"
    />
    {#if showCloud && view === null && !confirming}
      <Button
        label="hub.restore_pending.cloud_option"
        disabled={!released}
        disabledReason="hub.restore_pending.waiting"
        onclick={() => (confirming = true)}
      />
    {/if}
  </div>

  {#if confirming}
    <div class="panel" role="group" aria-labelledby="cloud-confirm" data-testid="cloud-confirm">
      <p class="type-hub-body" id="cloud-confirm">
        {t("hub.restore_pending.cloud_confirm", { branch: branchName })}
      </p>
      <div class="options">
        <Button
          label="hub.restore_pending.cloud_download"
          onclick={() => void act(startRestoreCloud)}
        />
        <Button
          label="hub.restore_pending.cloud_not_now"
          variant="secondary"
          onclick={() => (confirming = false)}
        />
      </div>
    </div>
  {/if}

  {#if rebuild && rebuild.phase !== "failed"}
    <div class="panel" data-testid="rebuild-progress">
      {#if rebuild.phase === "restarting"}
        <p class="type-hub-body" role="status" aria-live="polite" data-testid="rebuild-restarting">
          {t("hub.restore_pending.rebuild_restarting")}
        </p>
      {:else}
        <p class="type-hub-body" role="status" aria-live="polite" data-testid="rebuild-phase">
          {t(
            rebuild.phase === "replaying"
              ? "hub.restore_pending.rebuild_replaying"
              : "hub.restore_pending.rebuild_checking",
            { done: rebuild.done, total: rebuild.total },
          )}
        </p>
        <progress
          max="100"
          value={rebuild.percent}
          aria-label={t("hub.restore_pending.rebuild_progress_label")}
        ></progress>
        <p class="type-hub-body">
          {t("hub.restore_pending.rebuild_percent", { percent: rebuild.percent })}
        </p>
        <Button
          label="hub.restore_pending.rebuild_cancel"
          variant="secondary"
          onclick={() => void actRebuild(cancelRestoreRebuild)}
        />
      {/if}
    </div>
  {:else if rebuild?.phase === "failed"}
    <div class="panel" role="alert" data-testid="rebuild-failed">
      <p class="type-hub-body">
        {t("hub.restore_pending.rebuild_failed", { code: rebuild.error ?? "UNKNOWN" })}
      </p>
      <div class="options">
        <Button
          label="hub.restore_pending.rebuild_retry"
          onclick={() => void actRebuild(startRestoreRebuild)}
        />
        <Button
          label="hub.restore_pending.cloud_cancel"
          variant="secondary"
          onclick={() => void actRebuild(cancelRestoreRebuild)}
        />
      </div>
    </div>
  {:else if view?.phase === "downloading"}
    <div class="panel" data-testid="cloud-progress">
      <p class="type-hub-body" role="status" aria-live="polite">
        {t("hub.restore_pending.cloud_progress", { done: view.done, total: view.total })}
      </p>
      <progress
        max="100"
        value={view.percent}
        aria-label={t("hub.restore_pending.cloud_progress_label")}
      ></progress>
      <p class="type-hub-body">
        {t("hub.restore_pending.cloud_progress_detail", {
          size: formatBytes(view.bytes),
          percent: view.percent,
        })}
      </p>
      {#if view.retrying}
        <p class="type-hub-body" data-testid="cloud-retrying">
          {t("hub.restore_pending.cloud_retrying")}
        </p>
      {/if}
      <Button
        label="hub.restore_pending.cloud_cancel"
        variant="secondary"
        onclick={() => void act(cancelRestoreCloud)}
      />
    </div>
  {:else if view?.phase === "gap" && gap}
    <div class="panel" role="alert" data-testid="cloud-gap">
      <h2 class="type-hub-body">{t("hub.restore_pending.cloud_gap_title")}</h2>
      <p class="type-hub-body">
        {t("hub.restore_pending.cloud_gap", { from: gap.from_seq, to: gap.to_seq })}
      </p>
      <p class="type-hub-body">
        {t("hub.restore_pending.cloud_gap_cost", { last_good: gap.last_good_seq, lost: gap.lost })}
      </p>
      <div class="options">
        <Button
          label="hub.restore_pending.cloud_gap_partial"
          variant="warning"
          onclick={() => void act(restoreCloudPartial)}
        />
        <Button
          label="hub.restore_pending.cloud_cancel"
          variant="secondary"
          onclick={() => void act(cancelRestoreCloud)}
        />
      </div>
    </div>
  {:else if view?.phase === "ready" && ready}
    <div class="panel" data-testid="cloud-ready">
      <p class="type-hub-body">
        {ready.snapshot_date
          ? t("hub.restore_pending.cloud_ready", {
              date: ready.snapshot_date,
              batches: ready.batches,
              last_seq: ready.last_seq,
            })
          : t("hub.restore_pending.cloud_ready_no_snapshot", {
              batches: ready.batches,
              last_seq: ready.last_seq,
            })}
      </p>
      {#if ready.partial}
        <p class="type-hub-body">{t("hub.restore_pending.cloud_ready_partial")}</p>
      {/if}
      {#if rebuildConfirming}
        <div
          class="panel"
          role="group"
          aria-labelledby="rebuild-confirm"
          data-testid="rebuild-confirm"
        >
          <p class="type-hub-body" id="rebuild-confirm">
            {t("hub.restore_pending.rebuild_confirm")}
          </p>
          <div class="options">
            <Button
              label="hub.restore_pending.rebuild_go"
              onclick={() => void actRebuild(startRestoreRebuild)}
            />
            <Button
              label="hub.restore_pending.cloud_not_now"
              variant="secondary"
              onclick={() => (rebuildConfirming = false)}
            />
          </div>
        </div>
      {:else}
        <div class="options">
          <Button
            label="hub.restore_pending.rebuild_option"
            onclick={() => (rebuildConfirming = true)}
          />
          <Button
            label="hub.restore_pending.cloud_remove"
            variant="secondary"
            onclick={() => void act(cancelRestoreCloud)}
          />
        </div>
      {/if}
    </div>
  {:else if view?.phase === "failed"}
    <div class="panel" role="alert" data-testid="cloud-failed">
      <p class="type-hub-body">
        {t("hub.restore_pending.cloud_failed", { code: view.error ?? "UNKNOWN" })}
      </p>
      <div class="options">
        <Button
          label="hub.restore_pending.cloud_retry"
          onclick={() => void act(startRestoreCloud)}
        />
        <Button
          label="hub.restore_pending.cloud_cancel"
          variant="secondary"
          onclick={() => void act(cancelRestoreCloud)}
        />
      </div>
    </div>
  {/if}

  {#if problem && !busy}
    <p class="type-hub-body" role="alert" data-testid="cloud-problem">
      {problem === "HUB_UNREACHABLE"
        ? t("hub.restore_pending.cloud_unreachable")
        : t("hub.restore_pending.cloud_failed", { code: problem })}
    </p>
  {/if}
</section>

<style>
  .restore {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 60ch;
  }

  .options {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-4);
  }

  .panel {
    display: grid;
    justify-items: start;
    gap: var(--space-3);
  }

  progress {
    inline-size: min(100%, 40ch);
  }

  h1,
  h2,
  p {
    margin: 0;
  }
</style>
