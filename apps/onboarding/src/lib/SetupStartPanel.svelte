<script lang="ts">
  // "Start from <branch>'s setup" (Story 5.38): on by default when another paid branch of the
  // café has a saved setup, with a picker; otherwise it says how to carry the setup file over.
  import { Button, t } from "@arena/ui";
  import type { SetupStartView } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import { getSetupStart, retrySetupStart, setSetupStart, type ApiResult } from "./api";
  import { wizard } from "./wizard.svelte";

  const id = $props.id();
  const POLL_MS = 5000;

  let view = $state<SetupStartView | null>(null);
  let failed = $state(false);
  let busy = $state(false);

  const branchId = $derived(wizard.state?.branch?.id ?? "");
  const selected = $derived(view?.sources.find((s) => s.branch_id === view?.selected));
  const lastTryFailed = $derived(view?.enabled && view.status === "failed");

  function take(res: ApiResult<SetupStartView>) {
    failed = !res.ok;
    if (res.ok) view = res.data;
  }

  async function refresh() {
    if (branchId && !busy) take(await getSetupStart(branchId));
  }

  async function choose(enabled: boolean, source?: string) {
    if (!branchId || busy) return;
    busy = true;
    take(await setSetupStart(branchId, { enabled, source_branch_id: source }));
    busy = false;
  }

  async function retry() {
    if (!branchId || busy) return;
    busy = true;
    take(await retrySetupStart(branchId));
    busy = false;
  }

  onMount(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  });
</script>

{#if view && view.other_branches > 0}
  <section class="start" aria-labelledby="{id}-title" data-testid="setup-start">
    <h2 id="{id}-title">{t("onboarding.start.title")}</h2>
    {#if failed}
      <p class="form-error" role="alert">
        <span aria-hidden="true">⚠</span><span>{t("onboarding.start.error")}</span>
      </p>
    {/if}
    {#if view.sources.length === 0}
      <p class="muted" data-testid="setup-start-none">{t("onboarding.start.none")}</p>
      <p data-testid="setup-start-file-path">{t("onboarding.start.file_path")}</p>
    {:else}
      <div class="use">
        <span class="check">
          <input
            id="{id}-use"
            type="checkbox"
            checked={view.enabled}
            disabled={busy}
            aria-describedby="{id}-hint"
            data-testid="setup-start-use"
            onchange={(e) => void choose(e.currentTarget.checked)}
          />
          <span class="box" aria-hidden="true">{view.enabled ? "✓" : ""}</span>
        </span>
        <label for="{id}-use">
          {t("onboarding.start.use", { name: selected?.name ?? view.sources[0].name })}
        </label>
      </div>
      <p class="muted" id="{id}-hint">{t("onboarding.start.hint")}</p>
      {#if view.sources.length > 1}
        <label class="field">
          <span>{t("onboarding.start.pick")}</span>
          <select
            class="ui-interactive select"
            value={view.selected ?? ""}
            disabled={busy}
            data-testid="setup-start-pick"
            onchange={(e) => void choose(true, e.currentTarget.value)}
          >
            {#if !view.enabled}<option value="">—</option>{/if}
            {#each view.sources as s (s.branch_id)}
              <option value={s.branch_id}>{s.name}</option>
            {/each}
          </select>
        </label>
      {/if}
      {#if view.enabled}
        <p role="status" data-testid="setup-start-status">
          {#if view.status === "applied"}
            {t("onboarding.start.applied")}
          {:else if view.status === "retry"}
            {t("onboarding.start.queued")}
          {:else if view.status === "pending"}
            {t("onboarding.start.waiting")}
          {/if}
        </p>
      {/if}
      {#if lastTryFailed}
        <p class="form-error" role="alert" data-testid="setup-start-failed">
          <span aria-hidden="true">⚠</span>
          <span>{t("onboarding.start.failed")} {view.error ?? ""}</span>
        </p>
        <div class="actions">
          <Button
            label="onboarding.start.retry"
            onclick={retry}
            disabled={busy}
            disabledReason="onboarding.start.retrying"
          />
        </div>
        <p data-testid="setup-start-file-path">{t("onboarding.start.file_path")}</p>
      {/if}
    {/if}
  </section>
{/if}

<style>
  .start {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 32rem;
    margin: 0 0 var(--space-5);
    padding: var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
  }

  h2 {
    margin: 0;
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  p {
    margin: 0;
    line-height: var(--type-hub-label-line-height);
  }

  .muted {
    color: var(--text-muted);
  }

  .use {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .use label {
    font-weight: var(--type-hub-title-weight);
    cursor: pointer;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .select {
    --ui-bg: var(--text-field-background);
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
  }

  .check {
    position: relative;
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: var(--space-touch-min);
    height: var(--space-touch-min);
  }

  .check input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }

  .box {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    width: 1.5rem;
    height: 1.5rem;
    border: 2px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--text-field-background);
    color: var(--text-field-foreground);
    font-weight: var(--type-hub-title-weight);
    pointer-events: none;
  }

  .check:has(input:checked) .box {
    border-color: var(--accent);
  }

  .check:has(input:focus-visible) .box {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
