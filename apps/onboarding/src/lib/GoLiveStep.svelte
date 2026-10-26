<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { GoLiveBlocker, GoLiveItem, GoLiveView, WizardStep } from "@arena/ui/generated";
  import { PUBLIC_OWNER_URL } from "$app/env/public";
  import { onMount } from "svelte";
  import { getGoLive, goLive } from "./api";
  import { STEP_NAMES, advance, wizard } from "./wizard.svelte";

  const POLL_MS = 5000;
  const dashboard = PUBLIC_OWNER_URL.replace(/\/$/, "") + "/account/branches";
  let view = $state<GoLiveView | null>(null);
  let busy = $state(false);
  let error: MessageKey | null = $state(null);

  const ITEMS: Record<GoLiveItem, [MessageKey, MessageKey, MessageKey]> = {
    opening_hours: [
      "onboarding.golive.item.opening_hours",
      "onboarding.golive.where.opening_hours",
      "onboarding.golive.blocked.opening_hours",
    ],
    backup_location: [
      "onboarding.golive.item.backup_location",
      "onboarding.golive.where.backup_location",
      "onboarding.golive.blocked.backup_location",
    ],
    standby: [
      "onboarding.golive.item.standby",
      "onboarding.golive.where.standby",
      "onboarding.golive.blocked.opening_hours",
    ],
    ad_consent: [
      "onboarding.golive.item.ad_consent",
      "onboarding.golive.where.ad_consent",
      "onboarding.golive.blocked.ad_consent",
    ],
  };
  const STEPS: Record<WizardStep, MessageKey> = {
    cafe: "onboarding.golive.blocked.cafe",
    tier: "onboarding.golive.blocked.tier",
    install_hub: "onboarding.golive.blocked.install_hub",
    devices: "onboarding.golive.blocked.devices",
    rates: "onboarding.golive.blocked.rates",
    products: "onboarding.golive.blocked.products",
    staff: "onboarding.golive.blocked.staff",
    printer_payments: "onboarding.golive.blocked.printer_payments",
    test_session: "onboarding.golive.blocked.test_session",
    go_live: "onboarding.golive.button",
  };
  const blocked = (b: GoLiveBlocker): MessageKey =>
    "step" in b ? STEPS[b.step] : ITEMS[b.item][2];

  async function refresh() {
    const res = await getGoLive();
    if (res.ok) view = res.data;
  }

  async function go() {
    if (busy) return;
    busy = true;
    error = null;
    const res = await goLive();
    busy = false;
    if (!res.ok) {
      error = "onboarding.golive.error";
      return;
    }
    await refresh();
    await advance(res.data);
  }

  onMount(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  });

  const live = $derived(
    view?.live || wizard.state?.steps.some((s) => s.key === "go_live" && s.state === "done"),
  );
</script>

{#if live}
  <h1 tabindex="-1">{t("onboarding.golive.live.title")}</h1>
  <p>{t("onboarding.golive.live.body")}</p>
  <a class="link" href={dashboard} data-testid="golive-dashboard"
    >{t("onboarding.golive.live.open")}</a
  >
{:else}
  <h1 tabindex="-1">{t(STEP_NAMES.go_live)}</h1>
  <p>{t("onboarding.golive.intro")}</p>
  {#if view}
    <ul data-testid="golive-list">
      {#each view.items as item (item.key)}
        <li>
          <strong>{t(ITEMS[item.key][0])}</strong>
          {#if item.key === "standby"}
            <span>{t("onboarding.golive.recommended")}</span>
            {#if view.free}<span>{t("onboarding.golive.paid_only")}</span>{/if}
          {:else}
            <span>{t(item.done ? "onboarding.golive.done" : "onboarding.golive.todo")}</span>
          {/if}
          <small>{t(ITEMS[item.key][1])}</small>
        </li>
      {/each}
    </ul>
    {#if error}
      <p class="form-error" role="alert"><span aria-hidden="true">⚠</span> {t(error)}</p>
    {/if}
    <Button
      label="onboarding.golive.button"
      disabled={busy || !!view.blocker}
      disabledReason={view.blocker ? blocked(view.blocker) : "onboarding.golive.sending"}
      onclick={go}
    />
  {/if}
{/if}

<style>
  p {
    margin: 0 0 var(--space-4);
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0 0 var(--space-5);
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-2);
  }

  small {
    flex-basis: 100%;
  }

  .form-error {
    color: var(--text-field-error-text);
    font-weight: var(--type-hub-title-weight);
  }
</style>
