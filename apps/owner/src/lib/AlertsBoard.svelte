<script lang="ts">
  import { Button, TierLock, formatDateTime, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import {
    POLL_MS,
    alerts,
    loadSettings,
    olderAlerts,
    refreshAlerts,
    setWhatsapp,
  } from "./alerts.svelte";
  import CommandList from "./CommandList.svelte";
  import CommandPreview from "./CommandPreview.svelte";
  import { cancel } from "./commands.svelte";
  import { disablePush, enablePush, pushState, type PushState } from "./push";
  import SetupGate from "./SetupGate.svelte";
  import { isLive, shell } from "./shell.svelte";
  import ThresholdsForm from "./ThresholdsForm.svelte";

  const NOTES = {
    on: "owner.alerts.push.on",
    ios: "owner.alerts.push.ios",
    unsupported: "owner.alerts.push.unsupported",
    denied: "owner.alerts.push.denied",
    unavailable: "owner.alerts.push.unavailable",
  } as const satisfies Partial<Record<PushState, MessageKey>>;

  let push: PushState | null = $state(null);
  let busy = $state(false);
  let problem: MessageKey | "" = $state("");

  const key = $derived(alerts.settings?.vapid_public_key ?? null);
  const note = $derived(push && push !== "off" ? NOTES[push] : null);
  // A branch that is in setup or on Free raises no alerts: it says so, beside the one list.
  const gated = $derived((shell.branches ?? []).filter((b) => !isLive(b) || b.tier === "free"));
  const zoneOf = (branchId: string) => shell.branches?.find((b) => b.id === branchId)?.timezone;

  // How the push switch stands on this device, once the server's key is known.
  $effect(() => {
    if (!alerts.settings) return;
    void pushState(key).then((s) => (push = s));
  });

  async function togglePush(e: Event & { currentTarget: HTMLInputElement }) {
    const box = e.currentTarget;
    const wanted = box.checked;
    busy = true;
    problem = "";
    if (wanted) {
      const res = await enablePush(key);
      if (res === "failed") problem = "owner.alerts.push.failed";
      else push = res;
    } else if (await disablePush()) {
      push = "off";
    } else {
      problem = "owner.alerts.push.failed";
    }
    busy = false;
    box.checked = push === "on";
    void loadSettings();
  }

  async function toggleWhatsapp(e: Event & { currentTarget: HTMLInputElement }) {
    const box = e.currentTarget;
    busy = true;
    problem = "";
    if (!(await setWhatsapp(box.checked))) {
      problem = "owner.alerts.whatsapp.failed";
      box.checked = alerts.settings?.whatsapp ?? true;
    }
    busy = false;
  }

  // Reads when opened, every 30 s while visible, when the tab is focused again and when a push
  // message reaches the page.
  onMount(() => {
    void refreshAlerts();
    void loadSettings();
    const read = () => {
      if (document.visibilityState === "visible") void refreshAlerts();
    };
    const timer = setInterval(read, POLL_MS);
    const pushed = (e: MessageEvent) => {
      if (e.data?.type === "owner-alert") void refreshAlerts();
    };
    addEventListener("focus", read);
    document.addEventListener("visibilitychange", read);
    navigator.serviceWorker?.addEventListener("message", pushed);
    return () => {
      clearInterval(timer);
      removeEventListener("focus", read);
      document.removeEventListener("visibilitychange", read);
      navigator.serviceWorker?.removeEventListener("message", pushed);
      cancel();
    };
  });
</script>

<div class="board" data-component="alerts-board">
  {#if gated.length}
    <ul class="gates">
      {#each gated as b (b.id)}
        <li>
          <h2 class="type-hub-label">{b.name}</h2>
          {#if !isLive(b)}
            <SetupGate branch={b} />
          {:else}
            <TierLock tier="Basic" />
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  <section class="settings" aria-labelledby="alerts-settings-title">
    <h2 id="alerts-settings-title" class="type-hub-title">{t("owner.alerts.settings.title")}</h2>
    {#if alerts.settings}
      <label class="switch">
        <input
          type="checkbox"
          role="switch"
          checked={push === "on"}
          disabled={busy || push === null || push === "unsupported" || push === "unavailable"}
          onchange={togglePush}
        />
        <span>{t("owner.alerts.push.label")}</span>
      </label>
      {#if note}<p class="note type-hub-micro" data-push={push}>{t(note)}</p>{/if}
      <label class="switch">
        <input
          type="checkbox"
          role="switch"
          checked={alerts.settings.whatsapp}
          disabled={busy}
          onchange={toggleWhatsapp}
        />
        <span>{t("owner.alerts.whatsapp.label")}</span>
      </label>
      {#if problem}<p class="note type-hub-micro" role="alert">{t(problem)}</p>{/if}
    {:else if alerts.settingsFailed}
      <p>{t("owner.unavailable")}</p>
      <Button label="common.retry" variant="secondary" onclick={loadSettings} />
    {:else}
      <p>{t("owner.loading")}</p>
    {/if}
  </section>

  <ThresholdsForm />
  <CommandPreview />
  <CommandList />

  <section class="list" aria-labelledby="alerts-list-title">
    <h2 id="alerts-list-title" class="type-hub-title">{t("owner.alerts.title")}</h2>
    <div class="visually-hidden" role="status" aria-live="polite">{alerts.announcement}</div>
    {#if !alerts.loaded && alerts.failed}
      <p>{t("owner.unavailable")}</p>
      <Button label="common.retry" variant="secondary" onclick={refreshAlerts} />
    {:else if !alerts.loaded}
      <p>{t("owner.loading")}</p>
    {:else if !alerts.list.length}
      <p>{t("owner.alerts.empty")}</p>
    {:else}
      <ul class="rows">
        {#each alerts.list as a (a.id)}
          <li class="row" data-component="alert-row" data-kind={a.kind}>
            <span class="type-hub-label">{a.branch_name}</span>
            <a class="text" href={a.url}>{a.text}</a>
            <time class="when type-hub-micro" datetime={new Date(a.created_at).toISOString()}>
              {formatDateTime(a.created_at, zoneOf(a.branch_id))}
            </time>
          </li>
        {/each}
      </ul>
      {#if alerts.more}
        <Button label="owner.alerts.more" variant="secondary" onclick={olderAlerts} />
      {/if}
    {/if}
  </section>
</div>

<style>
  .board,
  .settings,
  .list {
    display: grid;
    gap: var(--space-3);
    align-content: start;
  }
  .board {
    gap: var(--space-owner-section-gap);
  }
  h2,
  p {
    margin: 0;
  }
  .switch {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--space-touch-min);
  }
  .switch input {
    width: var(--space-touch-min);
    height: var(--space-touch-min);
    margin: 0;
  }
  .note {
    color: var(--text-muted);
  }
  .gates {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-4);
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--line);
  }
  .row {
    display: grid;
    gap: var(--space-1);
    padding: var(--space-4) var(--space-5);
    background-color: var(--surface);
    border-bottom: 1px solid var(--line);
  }
  .text {
    overflow-wrap: anywhere;
  }
  .when {
    color: var(--text-muted);
  }
</style>
