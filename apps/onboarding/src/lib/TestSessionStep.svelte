<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { sendDeviceCommand } from "./api";
  import HubAge from "./HubAge.svelte";
  import { watchSetup } from "./setupView.svelte";
  import { STEP_NAMES } from "./wizard.svelte";

  const HOLD_MS = 3000;
  let passed = $state(false);
  const w = watchSetup((next) => {
    passed = next.steps.some((s) => s.key === "test_session" && s.state === "done");
    return passed ? HOLD_MS : 0;
  });
  let picked = $state("");
  let busy = $state(false);
  let error: MessageKey | null = $state(null);

  const kiosks = $derived(
    w.view?.setup?.devices.filter((d) => d.kind === "pc" && d.group_id && d.status === "online") ??
      [],
  );
  const chosen = $derived(kiosks.some((d) => d.id === picked) ? picked : (kiosks[0]?.id ?? ""));
  const phase = $derived(w.view?.setup?.status?.test_session ?? "unset");
  const last = $derived(w.view?.commands.filter((c) => c.kind === "setup.test_session").at(-1));
  const sent = $derived(!!last && last.status !== "rejected");
  const refused = $derived(last?.status === "rejected" ? last.error : null);

  async function start() {
    if (busy || !chosen) return;
    busy = true;
    error = null;
    const res = await sendDeviceCommand("setup.test_session", { device_id: chosen });
    busy = false;
    if (!res.ok) error = "onboarding.test_session.error";
    else await w.refresh();
  }
</script>

<h1 tabindex="-1">{t(STEP_NAMES.test_session)}</h1>
<p>{t("onboarding.test_session.intro")}</p>
<HubAge view={w.view} />
{#if error}
  <p class="form-error" role="alert"><span aria-hidden="true">⚠</span> {t(error)}</p>
{/if}
{#if passed || phase === "passed"}
  <p role="status" data-testid="test-session-state">{t("onboarding.test_session.passed")}</p>
{:else if sent}
  <p role="status" data-testid="test-session-state">{t("onboarding.devices.waiting")}</p>
{:else if phase === "running"}
  <p role="status" data-testid="test-session-state">{t("onboarding.test_session.running")}</p>
{:else}
  {#if phase === "failed"}
    <p class="form-error" role="alert" data-testid="test-session-state">
      <span aria-hidden="true">⚠</span>
      {t("onboarding.test_session.failed")}
    </p>
  {/if}
  {#if refused}
    <p class="form-error" role="alert" data-testid="test-session-refused">
      <span aria-hidden="true">⚠</span>
      {refused.message}
    </p>
  {/if}
  {#if w.view?.setup && kiosks.length === 0}
    <p data-testid="test-session-none">{t("onboarding.test_session.none")}</p>
  {:else if kiosks.length}
    <div class="pick">
      <div class="field">
        <label for="test-session-pick">{t("onboarding.test_session.pick")}</label>
        <select
          id="test-session-pick"
          class="ui-interactive select"
          value={chosen}
          onchange={(e) => (picked = e.currentTarget.value)}
        >
          {#each kiosks as d (d.id)}<option value={d.id}>{d.label}</option>{/each}
        </select>
      </div>
      <Button
        label={phase === "failed" ? "common.retry" : "onboarding.test_session.start"}
        disabled={busy}
        disabledReason="onboarding.devices.sending"
        onclick={start}
      />
    </div>
  {/if}
{/if}

<style>
  p {
    margin: 0 0 var(--space-4);
  }

  .pick {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-4);
    max-width: 32rem;
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

  .form-error {
    display: flex;
    gap: var(--space-2);
    color: var(--text-field-error-text);
    font-weight: var(--type-hub-title-weight);
  }
</style>
