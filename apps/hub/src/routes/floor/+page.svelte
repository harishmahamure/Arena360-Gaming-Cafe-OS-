<script lang="ts">
  import { StatusBadge, t } from "@arena/ui";
  import type { PageData } from "./$types";

  let { data }: { data: PageData } = $props();

  const snapshot = $derived(data.hub.ok ? data.hub.snapshot : null);
  // No zones exist yet, so every device is unassigned.
  const unassigned = $derived(snapshot?.devices.filter((d) => d.zone === null) ?? []);
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.nav.floor") })}</title>
</svelte:head>

<div class="floor">
  <h1 class="type-hub-numeric">{t("hub.nav.floor")}</h1>

  {#if snapshot && !snapshot.branch}
    <section class="note" data-testid="floor-not-set-up">
      <p class="type-hub-title">{t("hub.floor.not_set_up")}</p>
      <p class="type-hub-body muted">{t("hub.floor.not_set_up_body")}</p>
    </section>
  {:else if snapshot}
    <p class="type-hub-body muted" data-testid="floor-no-zones">{t("hub.floor.no_zones")}</p>

    <section class="unassigned" aria-labelledby="floor-unassigned-title">
      <h2 class="type-hub-title" id="floor-unassigned-title">{t("hub.floor.unassigned")}</h2>
      {#if unassigned.length === 0}
        <p class="type-hub-body muted">{t("hub.floor.no_devices")}</p>
      {:else}
        <ul class="devices">
          {#each unassigned as device (device.device_id)}
            <li class="device" data-device-id={device.device_id}>
              <span class="label type-hub-title">{device.label}</span>
              <StatusBadge state="unassigned" />
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
</div>

<style>
  .floor {
    display: grid;
    gap: var(--space-5);
  }

  h1,
  h2,
  p {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .note {
    display: grid;
    gap: var(--space-2);
    max-width: 60ch;
  }

  .unassigned {
    display: grid;
    gap: var(--space-4);
  }

  .devices {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 220px), 1fr));
    gap: var(--space-4);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* An unassigned device: a 2px dashed state-unassigned outline and no state bar. */
  .device {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-hub-tile-padding);
    border: 2px dashed var(--state-unassigned);
    border-radius: var(--radius-md);
    background-color: var(--floor-tile-background);
    overflow-wrap: anywhere;
  }
</style>
