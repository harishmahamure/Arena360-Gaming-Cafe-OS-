<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { Device } from "./android";
  import { MIN_WEBVIEW_MAJOR, gbOf } from "./android";

  let {
    device,
    checking,
    oncheck,
  }: { device: Exclude<Device, { kind: "ok" }>; checking: boolean; oncheck: () => void } = $props();

  const title = $derived(device.kind === "spec" ? "hub.device.title" : "hub.webview.title");
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t(title) })}</title>
</svelte:head>

<section class="device" aria-labelledby="device-title" data-testid="hub-device">
  <h1 class="type-hub-numeric" id="device-title">{t(title)}</h1>
  {#if device.kind === "spec"}
    {#each device.failures as f (f.requirement)}
      <p class="type-hub-body" data-testid="device-{f.requirement}">
        {t(f.requirement === "ram" ? "hub.device.ram" : "hub.device.storage", {
          found: gbOf(f.found_bytes),
          needed: f.needed_gb,
        })}
      </p>
    {/each}
  {:else}
    <p class="type-hub-body" data-testid="device-webview">
      {t("hub.webview.body", { version: MIN_WEBVIEW_MAJOR })}
    </p>
  {/if}
  <Button
    label="hub.device.check_again"
    disabled={checking}
    disabledReason="hub.device.checking"
    onclick={oncheck}
  />
</section>

<style>
  .device {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 60ch;
  }

  h1,
  p {
    margin: 0;
  }
</style>
