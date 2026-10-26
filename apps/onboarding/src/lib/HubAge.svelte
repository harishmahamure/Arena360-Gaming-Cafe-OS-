<script lang="ts">
  import { StatusBadge, t } from "@arena/ui";
  import type { SetupDevicesView } from "@arena/ui/generated";
  import { age } from "./devices";

  let { view }: { view: SetupDevicesView | null } = $props();
  const ago = $derived(view ? age(view) : null);
</script>

{#if !view?.setup}
  <p role="status" data-testid="hub-waiting">{t("onboarding.hub.waiting")}</p>
{:else if !view.hub_online}
  <p class="offline" role="status" data-testid="hub-offline">
    <StatusBadge state="offline" label="onboarding.devices.hub_offline" />
    {#if ago !== null}
      <span>
        {ago < 1 ? t("onboarding.devices.as_of_now") : t("onboarding.devices.as_of", { n: ago })}
      </span>
    {/if}
  </p>
{/if}

<style>
  p {
    margin: 0 0 var(--space-4);
  }

  .offline {
    --status-badge-background: transparent;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
</style>
